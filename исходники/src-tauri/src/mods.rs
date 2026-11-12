use crate::mc::{find_profile, inst_dir, Cb, Profile};
use crate::util::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

const MR: &str = "https://api.modrinth.com/v2";
/// CurseForge запрашивается через сервер Noxel: ключ хранится только там и в лаунчере его нет.
const CF: &str = "https://api.curseforge.com/v1";

pub fn folder(kind: &str) -> R<&'static str> {
    match kind {
        "mod" => Ok("mods"),
        "resourcepack" => Ok("resourcepacks"),
        "shader" => Ok("shaderpacks"),
        _ => Err("неизвестный тип".into()),
    }
}

fn kind_dir(profile_id: &str, kind: &str) -> R<PathBuf> {
    safe_name(profile_id)?;
    let d = inst_dir(profile_id).join(folder(kind)?);
    fs::create_dir_all(&d).map_err(e)?;
    Ok(d)
}

// ── индекс установленного: откуда пришёл каждый файл ──

#[derive(Serialize, Deserialize, Clone, Default)]
struct Meta {
    #[serde(default)]
    version: String,
    project_id: String,
    title: String,
    icon: Option<String>,
    provider: String,
    kind: String,
}

fn index_path(profile_id: &str) -> PathBuf {
    inst_dir(profile_id).join(".noxel/index.json")
}

fn load_index(profile_id: &str) -> HashMap<String, Meta> {
    read_json(&index_path(profile_id)).unwrap_or_default()
}

fn save_index(profile_id: &str, m: &HashMap<String, Meta>) {
    write_json(&index_path(profile_id), m);
}

/// Моды профиля, поставленные через лаунчер (источник и id проекта): по ним друг соберёт такой же профиль.
pub fn pack_manifest(profile_id: &str) -> Vec<Value> {
    let dir = inst_dir(profile_id).join("mods");
    let idx = load_index(profile_id);
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for (file, m) in idx {
        if m.kind != "mod" || m.provider == "noxel" || !dir.join(&file).exists() || !seen.insert(m.project_id.clone()) {
            continue;
        }
        out.push(json!({"provider": m.provider, "id": m.project_id, "name": m.title}));
    }
    out
}

pub fn list(profile_id: &str, kind: &str) -> R<Value> {
    let dir = kind_dir(profile_id, kind)?;
    let idx = load_index(profile_id);
    let mut out = vec![];
    for f in fs::read_dir(&dir).map_err(e)?.flatten() {
        let file = f.file_name().to_string_lossy().to_string();
        let (base, enabled) = match file.strip_suffix(".disabled") {
            Some(b) => (b.to_string(), false),
            None => (file.clone(), true),
        };
        let md = f.metadata().ok();
        let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let ok = match kind {
            "mod" => base.ends_with(".jar"),
            _ => base.ends_with(".zip") || is_dir,
        };
        if !ok {
            continue;
        }
        let m = idx.get(&base).filter(|m| m.kind == kind);
        out.push(json!({
            "file": file, "name": base, "enabled": enabled,
            "size": md.as_ref().map(|m| m.len()).unwrap_or(0),
            "title": m.map(|m| m.title.clone()),
            "icon": m.and_then(|m| m.icon.clone()),
            "provider": m.map(|m| m.provider.clone()),
            "project_id": m.map(|m| m.project_id.clone()),
            "version": m.map(|m| m.version.clone()),
        }));
    }
    out.sort_by(|a, b| {
        let k = |v: &Value| v["title"].as_str().or(v["name"].as_str()).unwrap_or("").to_lowercase();
        k(a).cmp(&k(b))
    });
    Ok(Value::Array(out))
}

pub fn toggle(profile_id: &str, kind: &str, file: &str) -> R<()> {
    let dir = kind_dir(profile_id, kind)?;
    safe_name(file)?;
    let from = dir.join(file);
    let to = match file.strip_suffix(".disabled") {
        Some(b) => dir.join(b),
        None => dir.join(format!("{file}.disabled")),
    };
    fs::rename(from, to).map_err(e)
}

pub fn remove(profile_id: &str, kind: &str, file: &str) -> R<()> {
    let dir = kind_dir(profile_id, kind)?;
    safe_name(file)?;
    let p = dir.join(file);
    if p.is_dir() {
        fs::remove_dir_all(&p).map_err(e)?;
    } else {
        fs::remove_file(&p).map_err(e)?;
    }
    let base = file.strip_suffix(".disabled").unwrap_or(file);
    let mut idx = load_index(profile_id);
    if idx.remove(base).is_some() {
        save_index(profile_id, &idx);
    }
    Ok(())
}

pub fn loaders_of(loader: &str) -> Vec<&'static str> {
    match loader {
        "fabric" => vec!["fabric"],
        "quilt" => vec!["quilt", "fabric"],
        "forge" => vec!["forge"],
        "neoforge" => vec!["neoforge"],
        // OptiFine ставится как отдельная версия, поэтому моды Forge с ним несовместимы
        _ => vec![],
    }
}

fn hit(provider: &str, id: String, slug: &str, title: &str, desc: &str, author: &str, icon: Option<&str>, downloads: u64, url: String) -> Value {
    json!({"provider": provider, "id": id, "slug": slug, "title": title, "desc": desc, "author": author, "icon": icon, "downloads": downloads, "url": url})
}

// ── поиск ──

pub fn search(provider: &str, kind: &str, query: &str, profile: &Profile, sort: &str, offset: u32, client_only: bool) -> R<Value> {
    folder(kind)?;
    if kind == "mod" && matches!(profile.loader.as_str(), "vanilla" | "optifine") {
        return Err("У этого профиля нет загрузчика модов. Создай профиль с Fabric, Quilt, Forge или NeoForge.".into());
    }
    match provider {
        "modrinth" => {
            let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{kind}")]];
            facets.push(vec![format!("versions:{}", profile.mc)]);
            if kind == "mod" {
                facets.push(loaders_of(&profile.loader).iter().map(|l| format!("categories:{l}")).collect());
            }
            if client_only && kind == "mod" {
                // работают на любом сервере: клиентская часть нужна или необязательна, серверная не нужна
                facets.push(vec!["client_side:required".into(), "client_side:optional".into()]);
                facets.push(vec!["server_side:unsupported".into(), "server_side:optional".into()]);
            }
            let index = match sort {
                "downloads" | "follows" | "newest" | "updated" => sort,
                _ => "relevance",
            };
            let r: Value = agent()
                .get(&format!("{MR}/search"))
                .query("query", query)
                .query("facets", &serde_json::to_string(&facets).map_err(e)?)
                .query("limit", "20")
                .query("offset", &offset.to_string())
                .query("index", index)
                .call()
                .map_err(e)?
                .into_json()
                .map_err(e)?;
            let hits: Vec<Value> = r["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|h| {
                    hit(
                        "modrinth",
                        h["project_id"].as_str().unwrap_or("").into(),
                        h["slug"].as_str().unwrap_or(""),
                        h["title"].as_str().unwrap_or(""),
                        h["description"].as_str().unwrap_or(""),
                        h["author"].as_str().unwrap_or(""),
                        h["icon_url"].as_str(),
                        h["downloads"].as_u64().unwrap_or(0),
                        format!("https://modrinth.com/{kind}/{}", h["slug"].as_str().unwrap_or("")),
                    )
                })
                .collect();
            Ok(json!({"hits": annotate(hits, kind, &profile.id), "total": r["total_hits"]}))
        }
        "curseforge" => {
            let class_id = match kind {
                "mod" => 6,
                "resourcepack" => 12,
                _ => 6552,
            };
            let sort_field = match sort {
                "downloads" => "6",
                "newest" => "11",
                "updated" => "3",
                "follows" => "2",
                _ => "2",
            };
            let mut req = agent()
                .get(&format!("{CF}/mods/search"))
                .query("gameId", "432")
                .query("classId", &class_id.to_string())
                .query("searchFilter", query)
                .query("gameVersion", &profile.mc)
                .query("sortField", if query.is_empty() && sort == "relevance" { "2" } else { sort_field })
                .query("sortOrder", "desc")
                .query("pageSize", "20")
                .query("index", &offset.to_string());
            if kind == "mod" {
                req = req.query("modLoaderType", cf_loader(&profile.loader));
            }
            let r: Value = req.call().map_err(cf_err)?.into_json().map_err(e)?;
            let hits: Vec<Value> = r["data"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|h| {
                    hit(
                        "curseforge",
                        h["id"].to_string(),
                        h["slug"].as_str().unwrap_or(""),
                        h["name"].as_str().unwrap_or(""),
                        h["summary"].as_str().unwrap_or(""),
                        h["authors"][0]["name"].as_str().unwrap_or(""),
                        h["logo"]["thumbnailUrl"].as_str(),
                        h["downloadCount"].as_f64().unwrap_or(0.0) as u64,
                        h["links"]["websiteUrl"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            Ok(json!({"hits": annotate(hits, kind, &profile.id), "total": r["pagination"]["totalCount"]}))
        }
        _ => Err("неизвестный сервис".into()),
    }
}

fn cf_err(x: ureq::Error) -> String {
    match x {
        ureq::Error::Status(_, r) => r.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| "CurseForge недоступен".into()),
        o => format!("нет связи с сервером Noxel: {o}"),
    }
}

fn cf_loader(l: &str) -> &'static str {
    match l {
        "forge" => "1",
        "fabric" => "4",
        "quilt" => "5",
        "neoforge" => "6",
        _ => "0",
    }
}

// ── установка с зависимостями ──

fn remember(profile_id: &str, file: &str, m: Meta) {
    let mut idx = load_index(profile_id);
    // обновление: убираем старый файл этого же проекта
    let old: Vec<String> = idx.iter().filter(|(k, v)| v.project_id == m.project_id && v.kind == m.kind && k.as_str() != file).map(|(k, _)| k.clone()).collect();
    if let Ok(dir) = kind_dir(profile_id, &m.kind) {
        for k in old {
            let _ = fs::remove_file(dir.join(&k));
            let _ = fs::remove_file(dir.join(format!("{k}.disabled")));
            idx.remove(&k);
        }
    }
    idx.insert(file.to_string(), m);
    save_index(profile_id, &idx);
}

/// Запоминает свой мод с сервера Noxel, чтобы он был подписан в списке установленного.
pub fn remember_noxel(profile_id: &str, file: &str, id: &str, title: &str) {
    let mut idx = load_index(profile_id);
    idx.retain(|f, m| !(m.provider == "noxel" && m.project_id == id && f != file));
    idx.insert(file.to_string(), Meta { project_id: id.into(), title: title.into(), icon: None, provider: "noxel".into(), kind: "mod".into(), version: String::new() });
    save_index(profile_id, &idx);
}

/// Файлы профиля, про которые известно, откуда они.
pub fn indexed_files(profile_id: &str) -> Vec<String> {
    load_index(profile_id).into_keys().collect()
}

/// Записывает источники сразу для нескольких файлов: (файл, id проекта Modrinth, название, иконка, версия, тип).
pub fn remember_many(profile_id: &str, items: Vec<(String, String, String, Option<String>, String, String)>) {
    if items.is_empty() {
        return;
    }
    let mut idx = load_index(profile_id);
    for (file, pid, title, icon, version, kind) in items {
        idx.insert(file, Meta { project_id: pid, title, icon, provider: "modrinth".into(), kind, version });
    }
    save_index(profile_id, &idx);
}


/// Известные id модов Fabric -> слаг на Modrinth.
const DEP_SLUGS: &[(&str, &str)] = &[
    ("yet_another_config_lib_v3", "yacl"),
    ("cloth-config", "cloth-config"),
    ("cloth_config", "cloth-config"),
    ("fabric-language-kotlin", "fabric-language-kotlin"),
    ("owo", "owo-lib"),
    ("architectury", "architectury-api"),
    ("midnightlib", "midnightlib"),
    ("geckolib", "geckolib"),
    ("puzzleslib", "puzzles-lib"),
    ("forgeconfigapiport", "forge-config-api-port"),
];

/// Слаги обязательных зависимостей jar (по fabric.mod.json), которых нет среди установленных модов.
fn jar_missing_deps(dir: &std::path::Path, jar: &std::path::Path) -> Vec<String> {
    let read = |p: &std::path::Path| -> Option<Value> {
        let mut z = zip::ZipArchive::new(fs::File::open(p).ok()?).ok()?;
        let mut f = z.by_name("fabric.mod.json").ok()?;
        let mut t = String::new();
        std::io::Read::read_to_string(&mut f, &mut t).ok()?;
        serde_json::from_str(&t).ok()
    };
    let Some(m) = read(jar) else { return vec![] };
    let Some(depends) = m["depends"].as_object() else { return vec![] };
    let mut have: HashSet<String> = HashSet::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for en in rd.flatten() {
            let pth = en.path();
            if pth.extension().map_or(false, |x| x == "jar") {
                if let Some(j) = read(&pth) {
                    if let Some(id) = j["id"].as_str() {
                        have.insert(id.to_string());
                    }
                    for pv in j["provides"].as_array().into_iter().flatten() {
                        if let Some(x) = pv.as_str() {
                            have.insert(x.to_string());
                        }
                    }
                }
            }
        }
    }
    depends
        .keys()
        .filter(|k| !have.contains(k.as_str()))
        .filter_map(|k| DEP_SLUGS.iter().find(|d| d.0 == k.as_str()).map(|d| d.1.to_string()))
        .collect()
}

pub fn already(profile_id: &str, kind: &str, project_id: &str) -> bool {
    let Ok(dir) = kind_dir(profile_id, kind) else { return false };
    load_index(profile_id).iter().any(|(f, m)| {
        m.project_id == project_id && m.kind == kind && (dir.join(f).exists() || dir.join(format!("{f}.disabled")).exists())
    })
}

pub fn install(provider: &str, kind: &str, project_id: &str, profile_id: &str, version: Option<String>, cb: Cb) -> R<Vec<String>> {
    let profile = find_profile(profile_id)?;
    if kind == "mod" && matches!(profile.loader.as_str(), "vanilla" | "optifine") {
        return Err("У этого профиля нет загрузчика модов.".into());
    }
    match provider {
        "modrinth" => install_mr(kind, project_id, &profile, version, cb),
        "curseforge" => install_cf(kind, project_id, &profile, version.and_then(|v| v.parse().ok()), cb),
        _ => Err("неизвестный сервис".into()),
    }
}

fn install_mr(kind: &str, project_id: &str, p: &Profile, first: Option<String>, cb: Cb) -> R<Vec<String>> {
    let dir = kind_dir(&p.id, kind)?;
    let loaders = loaders_of(&p.loader);
    let mut queue: Vec<(String, Option<String>)> = vec![(project_id.to_string(), first)];
    let mut seen: HashSet<String> = HashSet::new();
    let mut done = vec![];
    while let Some((pid, vid)) = queue.pop() {
        if !seen.insert(pid.clone()) {
            continue;
        }
        if !done.is_empty() && already(&p.id, kind, &pid) {
            continue;
        }
        let ver = match vid {
            Some(v) => get_json(&format!("{MR}/version/{v}")).ok(),
            None => None,
        };
        let ver = match ver {
            Some(v) => v,
            None => {
                let mut req = agent()
                    .get(&format!("{MR}/project/{pid}/version"))
                    .query("game_versions", &serde_json::to_string(&[&p.mc]).map_err(e)?);
                if kind == "mod" {
                    req = req.query("loaders", &serde_json::to_string(&loaders).map_err(e)?);
                }
                let list: Value = req.call().map_err(e)?.into_json().map_err(e)?;
                let arr = list.as_array().cloned().unwrap_or_default();
                arr.iter()
                    .find(|v| v["version_type"] == "release")
                    .or(arr.first())
                    .cloned()
                    .ok_or_else(|| format!("нет версии под {} {}", p.mc, crate::mc::loader_title(&p.loader)))?
            }
        };
        let files = ver["files"].as_array().cloned().unwrap_or_default();
        let f = files.iter().find(|f| f["primary"] == true).or(files.first()).ok_or("у версии нет файлов")?;
        let fname = safe_name(f["filename"].as_str().unwrap_or(""))?.to_string();
        let proj = get_json(&format!("{MR}/project/{pid}")).unwrap_or(json!({}));
        let title = proj["title"].as_str().unwrap_or(&fname).to_string();
        cb("Установка", -1.0, &title);
        download(f["url"].as_str().unwrap_or(""), &dir.join(&fname), f["hashes"]["sha1"].as_str())?;
        crate::store::dedupe_file(&dir.join(&fname));
        remember(
            &p.id,
            &fname,
            Meta { project_id: pid.clone(), title: title.clone(), icon: proj["icon_url"].as_str().map(String::from), provider: "modrinth".into(), kind: kind.into(), version: ver["version_number"].as_str().unwrap_or("").to_string() },
        );
        done.push(title);
        if kind == "mod" {
            // Modrinth часто не знает про обязательные зависимости: читаем их из самого jar
            for slug in jar_missing_deps(&dir, &dir.join(&fname)) {
                if let Ok(v) = get_json(&format!("{MR}/project/{slug}")) {
                    if let Some(id) = v["id"].as_str() {
                        if !seen.contains(id) && !already(&p.id, kind, id) {
                            queue.push((id.to_string(), None));
                        }
                    }
                }
            }
            for d in ver["dependencies"].as_array().cloned().unwrap_or_default() {
                if d["dependency_type"] != "required" {
                    continue;
                }
                let dpid = match d["project_id"].as_str() {
                    Some(x) => x.to_string(),
                    None => match d["version_id"].as_str().and_then(|v| get_json(&format!("{MR}/version/{v}")).ok()) {
                        Some(v) => v["project_id"].as_str().unwrap_or("").to_string(),
                        None => continue,
                    },
                };
                if !dpid.is_empty() {
                    queue.push((dpid, d["version_id"].as_str().map(String::from)));
                }
            }
        }
    }
    Ok(done)
}

fn install_cf(kind: &str, project_id: &str, p: &Profile, first_file: Option<u64>, cb: Cb) -> R<Vec<String>> {
    let dir = kind_dir(&p.id, kind)?;
    let mut queue = vec![project_id.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    let mut done = vec![];
    while let Some(pid) = queue.pop() {
        if !seen.insert(pid.clone()) {
            continue;
        }
        if !done.is_empty() && already(&p.id, kind, &pid) {
            continue;
        }
        let fixed = if done.is_empty() { first_file } else { None };
        let arr: Vec<Value> = match fixed {
            Some(fid) => {
                let r: Value = agent().get(&format!("{CF}/mods/{pid}/files/{fid}")).call().map_err(cf_err)?.into_json().map_err(e)?;
                vec![r["data"].clone()]
            }
            None => {
                let mut req = agent().get(&format!("{CF}/mods/{pid}/files")).query("gameVersion", &p.mc).query("pageSize", "20");
                if kind == "mod" {
                    req = req.query("modLoaderType", cf_loader(&p.loader));
                }
                let r: Value = req.call().map_err(cf_err)?.into_json().map_err(e)?;
                r["data"].as_array().cloned().unwrap_or_default()
            }
        };
        let f = arr
            .iter()
            .find(|f| fixed.is_some() || f["releaseType"] == 1)
            .or(arr.first())
            .ok_or_else(|| format!("нет файла под {} {}", p.mc, crate::mc::loader_title(&p.loader)))?;
        let fname = safe_name(f["fileName"].as_str().unwrap_or(""))?.to_string();
        let info: Value = agent()
            .get(&format!("{CF}/mods/{pid}"))
            .call()
            .map_err(cf_err)
            .and_then(|r| r.into_json().map_err(e))
            .unwrap_or(json!({}));
        let title = info["data"]["name"].as_str().unwrap_or(&fname).to_string();
        cb("Установка", -1.0, &title);
        let fid = f["id"].as_u64().unwrap_or(0);
        let url = match f["downloadUrl"].as_str() {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => format!("https://edge.forgecdn.net/files/{}/{}/{}", fid / 1000, fid % 1000, fname),
        };
        let sha1 = f["hashes"]
            .as_array()
            .and_then(|h| h.iter().find(|x| x["algo"] == 1))
            .and_then(|x| x["value"].as_str())
            .map(String::from);
        download(&url, &dir.join(&fname), sha1.as_deref())?;
        crate::store::dedupe_file(&dir.join(&fname));
        remember(
            &p.id,
            &fname,
            Meta {
                project_id: pid.clone(),
                title: title.clone(),
                icon: info["data"]["logo"]["thumbnailUrl"].as_str().map(String::from),
                provider: "curseforge".into(),
                kind: kind.into(),
                version: f["displayName"].as_str().unwrap_or("").to_string(),
            },
        );
        done.push(title);
        if kind == "mod" {
            for d in f["dependencies"].as_array().cloned().unwrap_or_default() {
                if d["relationType"] == 3 {
                    queue.push(d["modId"].to_string());
                }
            }
        }
    }
    Ok(done)
}

// ── сборки Noxel ──

/// (id, название, описание, слаги Modrinth). Префикс `shader:` или `rp:` ставит шейдер или ресурспак.
/// Только клиентские моды, которые не дают игрового преимущества и разрешены на обычных серверах.
const PRESETS: &[(&str, &str, &str, &[&str])] = &[
    ("base", "Noxel Base", "Fabric API, Mod Menu, Sodium, Iris Shaders: готово к шейдерам", &["fabric-api", "modmenu", "sodium", "iris"]),
    ("fps", "Оптимизация", "Максимум кадров на слабом ПК: Sodium, Lithium, FerriteCore, Entity Culling, ImmediatelyFast, ModernFix", &["fabric-api", "sodium", "lithium", "ferrite-core", "entityculling", "immediatelyfast", "modernfix", "modmenu"]),
    ("full", "Noxel Full", "Base + Lithium, FerriteCore, Entity Culling, Mod Menu", &["fabric-api", "modmenu", "sodium", "iris", "lithium", "ferrite-core", "entityculling"]),
    ("pvp", "PvP", "Оптимизация + зум, сытость и броня в интерфейсе, удобный F3, понятные подсказки: всё разрешено на серверах", &["fabric-api", "sodium", "lithium", "ferrite-core", "entityculling", "immediatelyfast", "modmenu", "zoomify", "appleskin", "betterf3", "shulkerboxtooltip", "no-chat-reports"]),
    ("pretty", "Красиво", "Шейдеры Complementary, динамический свет, 3D-слои скина, анимации, падающие листья, соединённое стекло", &["fabric-api", "sodium", "iris", "modmenu", "lambdynamiclights", "3dskinlayers", "wavey-capes", "not-enough-animations", "falling-leaves", "continuity", "entity-model-features", "entitytexturefeatures", "particle-rain", "shader:complementary-reimagined"]),
];

pub fn presets() -> Value {
    json!(PRESETS.iter().map(|(id, name, desc, mods)| json!({"id": id, "name": name, "desc": desc, "count": mods.len()})).collect::<Vec<_>>())
}

/// Ставит моды сборки в профиль. Возвращает (установлено, пропущено с причиной).
pub fn apply_preset(profile_id: &str, preset: &str, cb: Cb) -> R<(Vec<String>, Vec<String>)> {
    let (_, _, _, slugs) = PRESETS.iter().find(|p| p.0 == preset).ok_or("неизвестная сборка")?;
    let p = find_profile(profile_id)?;
    let (mut ok, mut skipped) = (vec![], vec![]);
    for entry in slugs.iter() {
        let (kind, slug) = match entry.split_once(':') {
            Some(("shader", s)) => ("shader", s),
            Some(("rp", s)) => ("resourcepack", s),
            _ => ("mod", *entry),
        };
        if kind == "mod" && matches!(p.loader.as_str(), "vanilla" | "optifine") {
            continue;
        }
        let id = match get_json(&format!("{MR}/project/{slug}")) {
            Ok(v) => v["id"].as_str().unwrap_or(slug).to_string(),
            Err(x) => {
                skipped.push(format!("{slug}: {x}"));
                continue;
            }
        };
        if already(&p.id, kind, &id) {
            continue;
        }
        match install_mr(kind, &id, &p, None, cb) {
            Ok(v) => ok.extend(v),
            Err(x) => skipped.push(format!("{slug}: {x}")),
        }
    }
    Ok((ok, skipped))
}

// ── свои файлы ──

pub fn add_local(profile_id: &str, kind: &str, paths: &[String]) -> R<Vec<String>> {
    let dir = kind_dir(profile_id, kind)?;
    let mut added = vec![];
    for src in paths {
        let src = PathBuf::from(src);
        let name = src.file_name().and_then(|n| n.to_str()).ok_or("неверный путь")?.to_string();
        safe_name(&name)?;
        let lower = name.to_lowercase();
        let ok = match kind {
            "mod" => lower.ends_with(".jar"),
            _ => lower.ends_with(".zip"),
        };
        if !ok {
            return Err(format!("«{name}»: для этого раздела нужен файл {}", if kind == "mod" { ".jar" } else { ".zip" }));
        }
        if !src.is_file() {
            return Err(format!("«{name}» не найден"));
        }
        fs::copy(&src, dir.join(&name)).map_err(e)?;
        let title = name.rsplit_once('.').map(|x| x.0).unwrap_or(&name).to_string();
        remember(profile_id, &name, Meta { project_id: format!("local:{name}"), title, icon: None, provider: "local".into(), kind: kind.into(), version: String::new() });
        added.push(name);
    }
    Ok(added)
}


// ── где мод уже скачан, описание, обновления ──

/// project_id → профили, где этот проект уже лежит в папке.
fn installed_map(kind: &str) -> HashMap<String, Vec<Value>> {
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    let Ok(folder) = folder(kind) else { return out };
    for p in crate::mc::load_profiles() {
        let dir = inst_dir(&p.id).join(folder);
        for (file, m) in load_index(&p.id) {
            if m.kind != kind || !(dir.join(&file).exists() || dir.join(format!("{file}.disabled")).exists()) {
                continue;
            }
            out.entry(m.project_id.clone()).or_default().push(json!({"profile": p.id, "name": p.name, "mc": p.mc, "loader": p.loader, "version": m.version}));
        }
    }
    out
}

fn annotate(mut hits: Vec<Value>, kind: &str, _current: &str) -> Vec<Value> {
    let map = installed_map(kind);
    for h in hits.iter_mut() {
        let id = h["id"].as_str().unwrap_or("").to_string();
        h["installed_in"] = json!(map.get(&id).cloned().unwrap_or_default());
    }
    hits
}

fn strip_html(s: &str) -> String {
    let (mut out, mut tag, mut name) = (String::new(), false, String::new());
    for c in s.chars() {
        match c {
            '<' => {
                tag = true;
                name.clear();
            }
            '>' if tag => {
                tag = false;
                let n = name.trim_start_matches('/').to_lowercase();
                if matches!(n.as_str(), "p" | "br" | "li" | "div" | "h1" | "h2" | "h3" | "h4" | "tr") {
                    out.push('\n');
                }
            }
            _ if tag => name.push(c),
            _ => out.push(c),
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

pub fn details(provider: &str, id: &str, profile_id: &str, kind: &str) -> R<Value> {
    let p = find_profile(profile_id)?;
    let installed = installed_map(kind).get(id).cloned().unwrap_or_default();
    match provider {
        "modrinth" => {
            let pr = get_json(&format!("{MR}/project/{id}"))?;
            let pid = pr["id"].as_str().unwrap_or(id).to_string();
            let mut req = agent().get(&format!("{MR}/project/{pid}/version")).query("game_versions", &serde_json::to_string(&[&p.mc]).map_err(e)?);
            if kind == "mod" {
                req = req.query("loaders", &serde_json::to_string(&loaders_of(&p.loader)).map_err(e)?);
            }
            let vs: Value = req.call().map_err(e)?.into_json().map_err(e)?;
            let versions: Vec<Value> = vs
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .take(12)
                .map(|v| json!({"provider": "modrinth", "vid": v["id"], "name": v["name"], "number": v["version_number"], "type": v["version_type"], "date": v["date_published"], "downloads": v["downloads"],
                                "loaders": v["loaders"], "games": v["game_versions"].as_array().map(|g| g.iter().take(6).cloned().collect::<Vec<_>>())}))
                .collect();
            let gallery: Vec<Value> = pr["gallery"].as_array().cloned().unwrap_or_default().iter().take(8).map(|g| json!({"url": g["url"], "title": g["title"]})).collect();
            let owner = get_json(&format!("{MR}/project/{pid}/members")).ok().and_then(|m| m.as_array().and_then(|a| a.iter().find(|x| x["role"] == "Owner").or(a.first()).cloned())).map(|m| m["user"]["username"].clone());
            Ok(json!({
                "provider": "modrinth", "id": pid, "title": pr["title"], "desc": pr["description"], "body": pr["body"], "body_kind": "md",
                "icon": pr["icon_url"], "gallery": gallery, "categories": pr["categories"], "downloads": pr["downloads"], "followers": pr["followers"],
                "license": pr["license"]["name"], "updated": pr["updated"], "client_side": pr["client_side"], "server_side": pr["server_side"],
                "author": owner, "url": format!("https://modrinth.com/{kind}/{}", pr["slug"].as_str().unwrap_or("")),
                "links": {"source": pr["source_url"], "issues": pr["issues_url"], "wiki": pr["wiki_url"], "discord": pr["discord_url"]},
                "versions": versions, "installed_in": installed,
            }))
        }
        "curseforge" => {
            let get = |path: &str| -> R<Value> { agent().get(&format!("{CF}{path}")).call().map_err(cf_err)?.into_json().map_err(e) };
            let m = get(&format!("/mods/{id}"))?;
            let d = &m["data"];
            let desc = get(&format!("/mods/{id}/description")).ok().map(|v| strip_html(v["data"].as_str().unwrap_or(""))).unwrap_or_default();
            let gallery: Vec<Value> = d["screenshots"].as_array().cloned().unwrap_or_default().iter().take(8).map(|g| json!({"url": g["url"], "title": g["title"]})).collect();
            let files = get(&format!("/mods/{id}/files?gameVersion={}&pageSize=10{}", p.mc, if kind == "mod" { format!("&modLoaderType={}", cf_loader(&p.loader)) } else { String::new() })).ok();
            let versions: Vec<Value> = files
                .as_ref()
                .and_then(|f| f["data"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .map(|f| json!({"provider": "curseforge", "vid": f["id"].to_string(), "name": f["displayName"], "number": f["fileName"], "type": match f["releaseType"].as_u64() { Some(1) => "release", Some(2) => "beta", _ => "alpha" }, "date": f["fileDate"], "downloads": f["downloadCount"], "games": f["gameVersions"]}))
                .collect();
            Ok(json!({
                "provider": "curseforge", "id": id, "title": d["name"], "desc": d["summary"], "body": desc, "body_kind": "text",
                "icon": d["logo"]["thumbnailUrl"], "gallery": gallery, "categories": d["categories"].as_array().map(|c| c.iter().map(|x| x["name"].clone()).collect::<Vec<_>>()),
                "downloads": d["downloadCount"], "updated": d["dateModified"], "author": d["authors"][0]["name"], "url": d["links"]["websiteUrl"],
                "links": {"source": d["links"]["sourceUrl"], "issues": d["links"]["issuesUrl"], "wiki": d["links"]["wikiUrl"]},
                "versions": versions, "installed_in": installed,
            }))
        }
        _ => Err("неизвестный сервис".into()),
    }
}

/// Сверяет установленные моды Modrinth с последними версиями под эту версию игры и загрузчик.
pub fn check_updates(profile_id: &str) -> R<Value> {
    use sha1::{Digest, Sha1};
    let p = find_profile(profile_id)?;
    let dir = kind_dir(profile_id, "mod")?;
    let idx = load_index(profile_id);
    let mut by_hash: HashMap<String, (String, Meta)> = HashMap::new();
    for f in fs::read_dir(&dir).map_err(e)?.flatten() {
        let file = f.file_name().to_string_lossy().to_string();
        let base = file.strip_suffix(".disabled").unwrap_or(&file).to_string();
        if !base.ends_with(".jar") {
            continue;
        }
        let Some(m) = idx.get(&base).filter(|m| m.provider == "modrinth" && m.kind == "mod") else { continue };
        let Ok(bytes) = fs::read(f.path()) else { continue };
        by_hash.insert(format!("{:x}", Sha1::digest(&bytes)), (base, m.clone()));
    }
    if by_hash.is_empty() {
        return Ok(json!([]));
    }
    let body = json!({"hashes": by_hash.keys().collect::<Vec<_>>(), "algorithm": "sha1", "loaders": loaders_of(&p.loader), "game_versions": [p.mc]});
    let r: Value = agent().post(&format!("{MR}/version_files/update")).send_json(body).map_err(e)?.into_json().map_err(e)?;
    let mut out = vec![];
    for (h, (file, m)) in &by_hash {
        let Some(v) = r.get(h) else { continue };
        let files = v["files"].as_array().cloned().unwrap_or_default();
        let newest = files.iter().find(|f| f["primary"] == true).or(files.first());
        let new_sha = newest.and_then(|f| f["hashes"]["sha1"].as_str()).unwrap_or("");
        if !new_sha.is_empty() && new_sha != h {
            out.push(json!({"file": file, "title": m.title, "project_id": m.project_id, "current": m.version, "new": v["version_number"]}));
        }
    }
    Ok(json!(out))
}

/// Обновляет моды до последних версий (старый файл заменяется, зависимости доставляются).
pub fn update_mods(profile_id: &str, project_ids: &[String], cb: Cb) -> R<Vec<String>> {
    let _ = crate::store::snapshot(profile_id, "перед обновлением модов");
    let p = find_profile(profile_id)?;
    let mut done = vec![];
    for pid in project_ids {
        done.extend(install_mr("mod", pid, &p, None, cb)?);
    }
    Ok(done)
}


// ── два источника в одном каталоге ──

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Ищет сразу на Modrinth и CurseForge и сливает результаты: один и тот же мод показывается один раз, с обоими источниками.
pub fn search_all(kind: &str, query: &str, profile: &Profile, sort: &str, offset: u32, client_only: bool) -> R<Value> {
    folder(kind)?;
    if kind == "mod" && matches!(profile.loader.as_str(), "vanilla" | "optifine") {
        return Err("У этого профиля нет загрузчика модов. Создай профиль с Fabric, Quilt, Forge или NeoForge.".into());
    }
    let skip_cf = client_only && kind == "mod"; // у CurseForge нет признака «работает только на клиенте»
    let (mr, cf) = std::thread::scope(|sc| {
        let a = sc.spawn(|| search("modrinth", kind, query, profile, sort, offset, client_only));
        let b = if skip_cf { None } else { Some(sc.spawn(|| search("curseforge", kind, query, profile, sort, offset, false))) };
        (a.join().unwrap(), b.map(|h| h.join().unwrap()))
    });
    let mut warns: Vec<String> = vec![];
    let mut total = 0u64;
    let mut take = |name: &str, r: R<Value>| -> Vec<Value> {
        match r {
            Ok(v) => {
                total += v["total"].as_u64().unwrap_or(0);
                v["hits"].as_array().cloned().unwrap_or_default()
            }
            Err(x) => {
                warns.push(format!("{name}: {x}"));
                vec![]
            }
        }
    };
    let mr_hits = take("Modrinth", mr);
    let cf_hits = match cf {
        Some(r) => take("CurseForge", r),
        None => vec![],
    };
    let mut out: Vec<Value> = vec![];
    let mut idx: HashMap<String, usize> = HashMap::new();
    for i in 0..mr_hits.len().max(cf_hits.len()) {
        for list in [&mr_hits, &cf_hits] {
            let Some(h) = list.get(i) else { continue };
            let src = json!({"provider": h["provider"], "id": h["id"], "url": h["url"]});
            let key = norm(h["title"].as_str().unwrap_or(""));
            match idx.get(&key).copied() {
                Some(k) if !key.is_empty() => {
                    let o = &mut out[k];
                    if let Some(a) = o["sources"].as_array_mut() {
                        a.push(src);
                    }
                    if h["downloads"].as_u64().unwrap_or(0) > o["downloads"].as_u64().unwrap_or(0) {
                        o["downloads"] = h["downloads"].clone();
                    }
                    if o["icon"].is_null() {
                        o["icon"] = h["icon"].clone();
                    }
                    if let (Some(dst), Some(add)) = (o["installed_in"].as_array().cloned(), h["installed_in"].as_array()) {
                        let mut all = dst;
                        for x in add {
                            if !all.iter().any(|y| y["profile"] == x["profile"]) {
                                all.push(x.clone());
                            }
                        }
                        o["installed_in"] = json!(all);
                    }
                }
                _ => {
                    let mut h2 = h.clone();
                    h2["sources"] = json!([src]);
                    idx.insert(key, out.len());
                    out.push(h2);
                }
            }
        }
    }
    if sort == "downloads" {
        out.sort_by(|a, b| b["downloads"].as_u64().cmp(&a["downloads"].as_u64()));
    }
    Ok(json!({"hits": out, "total": total, "warn": warns}))
}

/// Описание и версии со всех источников, где есть этот мод.
pub fn details_all(kind: &str, sources: &[(String, String)], profile_id: &str) -> R<Value> {
    let results: Vec<R<Value>> = std::thread::scope(|sc| {
        let hs: Vec<_> = sources.iter().map(|(p, id)| sc.spawn(move || details(p, id, profile_id, kind))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut warns = vec![];
    let mut oks: Vec<Value> = vec![];
    for (r, (p, _)) in results.into_iter().zip(sources) {
        match r {
            Ok(v) => oks.push(v),
            Err(x) => warns.push(format!("{}: {x}", if p == "modrinth" { "Modrinth" } else { "CurseForge" })),
        }
    }
    let mut base = oks.first().cloned().ok_or_else(|| warns.join("; "))?;
    let mut versions: Vec<Value> = oks.iter().flat_map(|v| v["versions"].as_array().cloned().unwrap_or_default()).collect();
    versions.sort_by(|a, b| b["date"].as_str().unwrap_or("").cmp(a["date"].as_str().unwrap_or("")));
    versions.truncate(30);
    let mut inst: Vec<Value> = vec![];
    for v in &oks {
        for x in v["installed_in"].as_array().cloned().unwrap_or_default() {
            if !inst.iter().any(|y| y["profile"] == x["profile"]) {
                inst.push(x);
            }
        }
    }
    base["versions"] = json!(versions);
    base["installed_in"] = json!(inst);
    base["sources"] = json!(oks.iter().map(|v| json!({"provider": v["provider"], "id": v["id"], "url": v["url"]})).collect::<Vec<_>>());
    base["warn"] = json!(warns);
    if base["gallery"].as_array().map(|g| g.is_empty()).unwrap_or(true) {
        if let Some(g) = oks.iter().find_map(|v| v["gallery"].as_array().filter(|g| !g.is_empty())) {
            base["gallery"] = json!(g);
        }
    }
    Ok(base)
}

/// Ставит список модов (slug Modrinth или id CurseForge): уже стоящие пропускает, недоступные для версии возвращает отдельно.
pub fn install_list(profile_id: &str, list: &[(String, String)], cb: Cb) -> R<(Vec<String>, Vec<String>)> {
    let p = find_profile(profile_id)?;
    if matches!(p.loader.as_str(), "vanilla" | "optifine") {
        return Err("У этого профиля нет загрузчика модов. Создай профиль с Fabric, Quilt, Forge или NeoForge.".into());
    }
    let (mut ok, mut skipped) = (vec![], vec![]);
    for (provider, id) in list {
        let real = if provider == "modrinth" { get_json(&format!("{MR}/project/{id}")).ok().and_then(|v| v["id"].as_str().map(String::from)).unwrap_or_else(|| id.clone()) } else { id.clone() };
        if already(&p.id, "mod", &real) {
            continue;
        }
        let r = if provider == "modrinth" { install_mr("mod", &real, &p, None, cb) } else { install_cf("mod", &real, &p, None, cb) };
        match r {
            Ok(v) => ok.extend(v),
            Err(x) => skipped.push(format!("{id}: {x}")),
        }
    }
    Ok((ok, skipped))
}
