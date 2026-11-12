//! Сборки модов (modpack) с Modrinth: поиск и установка как новый профиль (формат .mrpack).
use crate::mc;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

const MR: &str = "https://api.modrinth.com/v2";

const CF: &str = "https://api.curseforge.com/v1";

/// Поиск сборок сразу в двух каталогах. `mc` — версия игры (пусто: любая).
pub fn search(query: &str, sort: &str, offset: u32, mc: &str) -> R<Value> {
    let (mr, cf) = std::thread::scope(|sc| {
        let a = sc.spawn(|| search_mr(query, sort, offset, mc));
        let b = sc.spawn(|| search_cf(query, sort, offset, mc));
        (a.join().unwrap_or_else(|_| Err("поиск прервался".into())), b.join().unwrap_or_else(|_| Err("поиск прервался".into())))
    });
    if mr.is_err() && cf.is_err() {
        return Err(mr.err().unwrap_or_default());
    }
    let mut hits: Vec<Value> = mr.unwrap_or_default();
    hits.extend(cf.unwrap_or_default());
    let key = |h: &Value| h["downloads"].as_u64().unwrap_or(0);
    if sort == "downloads" || query.is_empty() {
        hits.sort_by_key(|h| std::cmp::Reverse(key(h)));
    } else {
        // по релевантности чередуем источники, чтобы оба были сверху
        let (m, c): (Vec<Value>, Vec<Value>) = hits.into_iter().partition(|h| h["provider"] == "modrinth");
        hits = m.iter().zip(c.iter().chain(std::iter::repeat(&Value::Null))).flat_map(|(a, b)| [a.clone(), b.clone()]).filter(|v| !v.is_null()).collect();
        hits.extend(m.iter().skip(c.len()).cloned());
        hits.extend(c.iter().skip(m.len()).cloned());
    }
    Ok(json!({"hits": hits}))
}

fn search_cf(query: &str, sort: &str, offset: u32, mc: &str) -> R<Vec<Value>> {
    let sort_field = match sort {
        "downloads" => "6",
        "newest" => "11",
        "updated" => "3",
        _ => if query.is_empty() { "6" } else { "2" },
    };
    let mut req = agent().get(&format!("{CF}/mods/search")).query("gameId", "432").query("classId", "4471").query("searchFilter", query).query("sortField", sort_field).query("sortOrder", "desc").query("pageSize", "20").query("index", &offset.to_string());
    if !mc.is_empty() {
        req = req.query("gameVersion", mc);
    }
    let r: Value = req.call().map_err(|x| format!("CurseForge: {x}"))?.into_json().map_err(e)?;
    Ok(r["data"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let mut loaders: Vec<String> = vec![];
            let mut mcv = Value::Null;
            for f in h["latestFilesIndexes"].as_array().cloned().unwrap_or_default() {
                if let Some(v) = f["gameVersion"].as_str() {
                    if mcv.is_null() || (!mc.is_empty() && v == mc) {
                        mcv = json!(v);
                    }
                }
            }
            for t in h["latestFiles"].as_array().and_then(|a| a.first()).and_then(|f| f["gameVersions"].as_array()).cloned().unwrap_or_default() {
                if let Some(l) = t.as_str().map(|x| x.to_lowercase()).filter(|x| ["fabric", "quilt", "forge", "neoforge"].contains(&x.as_str())) {
                    if !loaders.contains(&l) {
                        loaders.push(l);
                    }
                }
            }
            json!({"provider": "curseforge", "id": h["id"].to_string(), "slug": h["slug"], "title": h["name"], "desc": h["summary"], "author": h["authors"][0]["name"],
                   "icon": h["logo"]["thumbnailUrl"], "downloads": h["downloadCount"].as_f64().unwrap_or(0.0) as u64, "mc": mcv, "loaders": loaders,
                   "url": h["links"]["websiteUrl"]})
        })
        .collect())
}

fn search_mr(query: &str, sort: &str, offset: u32, mc: &str) -> R<Vec<Value>> {
    let mut f = vec![vec!["project_type:modpack".to_string()]];
    if !mc.is_empty() {
        f.push(vec![format!("versions:{mc}")]);
    }
    let facets = serde_json::to_string(&f).map_err(e)?;
    let index = match sort {
        "downloads" | "follows" | "newest" | "updated" => sort,
        _ if query.is_empty() => "downloads",
        _ => "relevance",
    };
    let r: Value = agent()
        .get(&format!("{MR}/search"))
        .query("query", query)
        .query("facets", &facets)
        .query("limit", "20")
        .query("offset", &offset.to_string())
        .query("index", index)
        .call()
        .map_err(e)?
        .into_json()
        .map_err(e)?;
    Ok(r["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let loaders: Vec<&str> = h["categories"].as_array().map(|c| c.iter().filter_map(|x| x.as_str()).filter(|x| ["fabric", "quilt", "forge", "neoforge"].contains(x)).collect()).unwrap_or_default();
            json!({
                "provider": "modrinth", "id": h["project_id"], "slug": h["slug"], "title": h["title"], "desc": h["description"], "author": h["author"],
                "icon": h["icon_url"], "downloads": h["downloads"], "mc": h["versions"].as_array().and_then(|v| v.last()).cloned().unwrap_or(Value::Null),
                "loaders": loaders, "url": format!("https://modrinth.com/modpack/{}", h["slug"].as_str().unwrap_or("")),
            })
        })
        .collect())
}

pub fn safe_rel(p: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() || p.contains("..") || p.contains('\0') || p.starts_with('/') || p.starts_with('\\') {
        return None;
    }
    Some(path.to_path_buf())
}

/// Скачивает сборку и создаёт под неё профиль. Игра и загрузчик доустановятся при первом запуске.
pub fn install(provider: &str, project_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    if provider == "curseforge" {
        install_cf(project_id, cb)
    } else {
        install_mr(project_id, cb)
    }
}

/// Параллельная загрузка, которая не падает целиком: возвращает список файлов, которые не скачались.
fn fetch_many(jobs: Vec<Job>, threads: usize, cb: &(dyn Fn(u64, u64) + Sync)) -> Vec<String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    let total = jobs.len() as u64;
    let queue = Mutex::new(std::collections::VecDeque::from(jobs));
    let failed = Mutex::new(vec![]);
    let done = AtomicU64::new(0);
    std::thread::scope(|sc| {
        for _ in 0..threads.max(1) {
            sc.spawn(|| loop {
                let Some(j) = queue.lock().unwrap().pop_front() else { break };
                if download(&j.url, &j.path, j.sha1.as_deref()).is_err() && download(&j.url, &j.path, None).is_err() {
                    failed.lock().unwrap().push(j.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                }
                cb(done.fetch_add(1, Ordering::Relaxed) + 1, total);
            });
        }
    });
    failed.into_inner().unwrap()
}

fn extract_overrides(z: &mut zip::ZipArchive<fs::File>, prefixes: &[String], dir: &Path) -> R<Vec<String>> {
    let mut out_files = vec![];
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(enc) = ent.enclosed_name() else { continue };
        let s = enc.to_string_lossy().replace('\\', "/");
        let Some(rel) = prefixes.iter().find_map(|p| s.strip_prefix(p.as_str())).filter(|r| !r.is_empty()) else { continue };
        let Some(rel) = safe_rel(rel) else { continue };
        let out = dir.join(&rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        let mut f = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut f).map_err(e)?;
        out_files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    Ok(out_files)
}

fn install_cf(project_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    cb("Ищу версию сборки", 0.0, "");
    let r: Value = agent().get(&format!("{CF}/mods/{project_id}/files")).query("pageSize", "20").call().map_err(|x| format!("CurseForge: {x}"))?.into_json().map_err(e)?;
    let list = r["data"].as_array().cloned().unwrap_or_default();
    // самая свежая стабильная версия, у которой есть скачивание
    let f = list.iter().filter(|x| x["isAvailable"] != false).max_by_key(|x| (x["releaseType"] == 1, x["id"].as_u64().unwrap_or(0))).ok_or("у сборки нет доступных файлов")?;
    let (fid, fname) = (f["id"].as_u64().ok_or("нет id файла")?, f["fileName"].as_str().unwrap_or("pack.zip"));
    let url = f["downloadUrl"].as_str().map(String::from).unwrap_or_else(|| format!("https://edge.forgecdn.net/files/{}/{}/{}", fid / 1000, fid % 1000, fname));
    let tmp = data_dir().join("cache").join(format!("pack-cf-{project_id}.zip"));
    cb("Скачиваю сборку", 0.05, "");
    download(&url, &tmp, None)?;
    let origin = json!({"provider": "curseforge", "project_id": project_id, "version": fid.to_string(), "version_name": f["displayName"].as_str().unwrap_or(fname)});
    let r = install_cf_file(&tmp, None, Some(origin), cb);
    let _ = fs::remove_file(&tmp);
    r
}

/// Ставит сборку CurseForge из zip-файла на диске (скачанного или выбранного пользователем).
pub fn install_cf_file(tmp: &Path, into: Option<&str>, origin: Option<Value>, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    let mut z = zip::ZipArchive::new(fs::File::open(tmp).map_err(e)?).map_err(e)?;
    let man: Value = {
        let mut ent = z.by_name("manifest.json").map_err(|_| "это не сборка CurseForge: нет manifest.json")?;
        let mut s = String::new();
        ent.read_to_string(&mut s).map_err(e)?;
        serde_json::from_str(s.trim_start_matches('\u{feff}')).map_err(e)?
    };
    let mcv = man["minecraft"]["version"].as_str().ok_or("в сборке не указана версия Minecraft")?;
    let ml = man["minecraft"]["modLoaders"].as_array().cloned().unwrap_or_default();
    let pick = ml.iter().find(|x| x["primary"] == true).or(ml.first()).and_then(|x| x["id"].as_str()).ok_or("в сборке не указан загрузчик модов")?;
    let (loader, lver) = ["neoforge", "fabric", "quilt", "forge"].iter().find_map(|l| pick.strip_prefix(&format!("{l}-")).map(|v| (l.to_string(), v.to_string()))).ok_or("сборка для неподдерживаемого загрузчика")?;
    let name = man["name"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("Сборка CurseForge");
    let profile = match into {
        Some(id) => mc::find_profile(id)?,
        None => mc::create_profile(mcv, &loader, Some(lver), name, None)?,
    };
    let dir = mc::inst_dir(&profile.id);

    // описания файлов (имя, ссылка, хеш) запрашиваем пачками через сервер Noxel
    let ids: Vec<u64> = man["files"].as_array().cloned().unwrap_or_default().iter().filter(|x| x["required"] != false).filter_map(|x| x["fileID"].as_u64()).collect();
    let mut jobs = vec![];
    let mut files: Vec<String> = vec![];
    for (n, chunk) in ids.chunks(200).enumerate() {
        cb("Получаю список модов", 0.08 + 0.04 * n as f64, "");
        let r: Value = agent().post(&format!("{CF}/mods/files")).send_json(json!({"fileIds": chunk})).map_err(|x| format!("CurseForge: {x}"))?.into_json().map_err(e)?;
        for d in r["data"].as_array().cloned().unwrap_or_default() {
            let (id, fname) = (d["id"].as_u64().unwrap_or(0), d["fileName"].as_str().unwrap_or(""));
            if fname.is_empty() || safe_rel(fname).is_none() {
                continue;
            }
            let u = d["downloadUrl"].as_str().map(String::from).unwrap_or_else(|| format!("https://edge.forgecdn.net/files/{}/{}/{}", id / 1000, id % 1000, fname));
            let sha1 = d["hashes"].as_array().and_then(|h| h.iter().find(|x| x["algo"] == 1)).and_then(|x| x["value"].as_str()).map(String::from);
            let sub = if fname.to_lowercase().ends_with(".zip") { "resourcepacks" } else { "mods" };
            files.push(format!("{sub}/{fname}"));
            jobs.push(Job { url: u, path: dir.join(sub).join(fname), sha1 });
        }
    }
    let total = jobs.len().max(1) as f64;
    cb("Скачиваю файлы сборки", 0.15, "");
    let failed = fetch_many(jobs, 8, &|d, _| cb("Скачиваю файлы сборки", 0.15 + 0.8 * d as f64 / total, &format!("{d} из {}", total as u64)));
    cb("Распаковываю настройки", 0.97, "");
    let ov = man["overrides"].as_str().unwrap_or("overrides").trim_matches('/').to_string();
    let ovf = extract_overrides(&mut z, &[format!("{ov}/")], &dir)?;
    files.extend(ovf);
    finish(&profile.id, origin, files);
    cb("Готово", 1.0, "");
    Ok(json!({"profile": profile.id, "name": profile.name, "mc": profile.mc, "loader": profile.loader, "failed": failed}))
}

/// Запоминает, откуда пришла сборка и какие файлы она положила: по этому списку работают обновление и откат.
fn finish(profile_id: &str, origin: Option<Value>, files: Vec<String>) {
    let dir = mc::inst_dir(profile_id);
    crate::store::dedupe_profile(profile_id);
    crate::store::index_unknown(profile_id);
    if let Some(mut o) = origin {
        o["files"] = json!(files);
        o["installed"] = json!(now());
        write_json(&dir.join(".noxel/pack.json"), &o);
    }
}

fn install_mr(project_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    cb("Ищу версию сборки", 0.0, "");
    let vs: Value = agent().get(&format!("{MR}/project/{project_id}/version")).call().map_err(e)?.into_json().map_err(e)?;
    let list = vs.as_array().ok_or("у сборки нет версий")?;
    let v = list.iter().find(|x| x["version_type"] == "release").or(list.first()).ok_or("у сборки нет версий")?;
    let file = v["files"].as_array().and_then(|f| f.iter().find(|x| x["primary"] == true).or(f.first())).ok_or("в версии сборки нет файла")?;
    let url = file["url"].as_str().ok_or("нет ссылки на сборку")?;
    let tmp = data_dir().join("cache").join(format!("pack-{project_id}.mrpack"));
    cb("Скачиваю сборку", 0.05, "");
    download(url, &tmp, file["hashes"]["sha1"].as_str())?;
    let origin = json!({"provider": "modrinth", "project_id": project_id, "version": v["id"], "version_name": v["version_number"]});
    let r = install_mrpack(&tmp, None, Some(origin), cb);
    let _ = fs::remove_file(&tmp);
    r
}

/// Ставит сборку .mrpack из файла на диске. `into` — поставить поверх существующего профиля (обновление сборки).
pub fn install_mrpack(tmp: &Path, into: Option<&str>, origin: Option<Value>, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    let mut z = zip::ZipArchive::new(fs::File::open(tmp).map_err(e)?).map_err(e)?;
    let idx: Value = {
        let mut ent = z.by_name("modrinth.index.json").map_err(|_| "это не сборка Modrinth (.mrpack)")?;
        let mut s = String::new();
        ent.read_to_string(&mut s).map_err(e)?;
        serde_json::from_str(&s).map_err(e)?
    };
    let deps = &idx["dependencies"];
    let mcv = deps["minecraft"].as_str().ok_or("в сборке не указана версия Minecraft")?;
    let (loader, lver) = [("fabric-loader", "fabric"), ("quilt-loader", "quilt"), ("neoforge", "neoforge"), ("forge", "forge")]
        .iter()
        .find_map(|(k, l)| deps[*k].as_str().map(|v| (l.to_string(), v.to_string())))
        .ok_or("сборка для неподдерживаемого загрузчика")?;
    let name = idx["name"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("Сборка Modrinth");
    let profile = match into {
        Some(id) => mc::find_profile(id)?,
        None => mc::create_profile(mcv, &loader, Some(lver), name, None)?,
    };
    let dir = mc::inst_dir(&profile.id);

    let mut jobs = vec![];
    let mut files: Vec<String> = vec![];
    for f in idx["files"].as_array().cloned().unwrap_or_default() {
        if f["env"]["client"] == "unsupported" {
            continue;
        }
        let Some(rel) = f["path"].as_str().and_then(safe_rel) else { continue };
        let Some(u) = f["downloads"][0].as_str() else { continue };
        files.push(rel.to_string_lossy().replace('\\', "/"));
        jobs.push(Job { url: u.to_string(), path: dir.join(rel), sha1: f["hashes"]["sha1"].as_str().map(String::from) });
    }
    let total = jobs.len().max(1) as f64;
    cb("Скачиваю файлы сборки", 0.1, "");
    download_all(jobs, 8, &|d, _| cb("Скачиваю файлы сборки", 0.1 + 0.85 * d as f64 / total, &format!("{d} из {}", total as u64)))?;

    cb("Распаковываю настройки", 0.97, "");
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(enc) = ent.enclosed_name() else { continue };
        let s = enc.to_string_lossy().replace('\\', "/");
        let rel = match s.strip_prefix("overrides/").or_else(|| s.strip_prefix("client-overrides/")) {
            Some(r) if !r.is_empty() => r.to_string(),
            _ => continue,
        };
        let Some(rel) = safe_rel(&rel) else { continue };
        let out = dir.join(&rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        let mut f = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut f).map_err(e)?;
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    finish(&profile.id, origin, files);
    cb("Готово", 1.0, "");
    Ok(json!({"profile": profile.id, "name": profile.name, "mc": profile.mc, "loader": profile.loader}))
}

// ── обновление установленной сборки с показом изменений и откатом ──

fn pack_origin(profile_id: &str) -> Option<Value> {
    read_json(&mc::inst_dir(profile_id).join(".noxel/pack.json"))
}

/// Свежая версия сборки под ту же версию игры: (id версии, название, ссылка, sha1, описание изменений).
fn latest_for(o: &Value, p: &mc::Profile) -> R<Option<(String, String, String, Option<String>, String)>> {
    let pid = o["project_id"].as_str().ok_or("нет id сборки")?;
    if o["provider"] == "curseforge" {
        let r: Value = agent().get(&format!("{CF}/mods/{pid}/files")).query("gameVersion", &p.mc).query("pageSize", "20").call().map_err(|x| format!("CurseForge: {x}"))?.into_json().map_err(e)?;
        let list = r["data"].as_array().cloned().unwrap_or_default();
        let Some(f) = list.iter().filter(|x| x["isAvailable"] != false).max_by_key(|x| (x["releaseType"] == 1, x["id"].as_u64().unwrap_or(0))) else { return Ok(None) };
        let fid = f["id"].as_u64().unwrap_or(0);
        let fname = f["fileName"].as_str().unwrap_or("pack.zip");
        let url = f["downloadUrl"].as_str().map(String::from).unwrap_or_else(|| format!("https://edge.forgecdn.net/files/{}/{}/{}", fid / 1000, fid % 1000, fname));
        return Ok(Some((fid.to_string(), f["displayName"].as_str().unwrap_or(fname).to_string(), url, None, String::new())));
    }
    let vs: Value = agent().get(&format!("{MR}/project/{pid}/version")).query("game_versions", &serde_json::to_string(&[&p.mc]).map_err(e)?).call().map_err(e)?.into_json().map_err(e)?;
    let list = vs.as_array().cloned().unwrap_or_default();
    let Some(v) = list.iter().find(|x| x["version_type"] == "release").or(list.first()) else { return Ok(None) };
    let file = v["files"].as_array().and_then(|f| f.iter().find(|x| x["primary"] == true).or(f.first())).ok_or("в версии сборки нет файла")?;
    Ok(Some((
        v["id"].as_str().unwrap_or("").to_string(),
        v["version_number"].as_str().unwrap_or("").to_string(),
        file["url"].as_str().unwrap_or("").to_string(),
        file["hashes"]["sha1"].as_str().map(String::from),
        v["changelog"].as_str().unwrap_or("").chars().take(1500).collect(),
    )))
}

fn pack_files_of(path: &Path, cf: bool) -> Vec<String> {
    let Ok(f) = fs::File::open(path) else { return vec![] };
    let Ok(mut z) = zip::ZipArchive::new(f) else { return vec![] };
    let mut s = String::new();
    if cf {
        if let Ok(mut ent) = z.by_name("manifest.json") {
            let _ = ent.read_to_string(&mut s);
        }
        let v: Value = serde_json::from_str(s.trim_start_matches('\u{feff}')).unwrap_or(json!({}));
        return v["files"].as_array().map(|a| a.iter().map(|x| format!("cf:{}", x["projectID"])).collect()).unwrap_or_default();
    }
    if let Ok(mut ent) = z.by_name("modrinth.index.json") {
        let _ = ent.read_to_string(&mut s);
    }
    let v: Value = serde_json::from_str(&s).unwrap_or(json!({}));
    v["files"].as_array().map(|a| a.iter().filter_map(|x| x["path"].as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Есть ли у сборки профиля новая версия и что в ней поменяется.
pub fn update_check(profile_id: &str) -> R<Value> {
    let p = mc::find_profile(profile_id)?;
    let Some(o) = pack_origin(profile_id) else { return Ok(json!({"pack": false})) };
    let cur = o["version"].as_str().map(String::from).unwrap_or_else(|| o["version"].to_string());
    let Some((vid, vname, url, sha1, changelog)) = latest_for(&o, &p)? else {
        return Ok(json!({"pack": true, "update": false, "current": o["version_name"]}));
    };
    if vid == cur {
        return Ok(json!({"pack": true, "update": false, "current": o["version_name"]}));
    }
    let cf = o["provider"] == "curseforge";
    let tmp = data_dir().join("cache").join(format!("pack-upd-{}.zip", mc::slug(profile_id)));
    download(&url, &tmp, sha1.as_deref())?;
    let new: Vec<String> = pack_files_of(&tmp, cf);
    let old: Vec<String> = o["files"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).filter(|f| f.starts_with("mods/") || f.starts_with("resourcepacks/") || f.starts_with("shaderpacks/")).collect()).unwrap_or_default();
    let name = |f: &str| f.rsplit('/').next().unwrap_or(f).to_string();
    let (added, removed): (Vec<String>, Vec<String>) = if cf {
        (vec![], vec![])
    } else {
        (new.iter().filter(|f| !old.contains(f)).map(|f| name(f)).collect(), old.iter().filter(|f| !new.contains(f)).map(|f| name(f)).collect())
    };
    Ok(json!({"pack": true, "update": true, "current": o["version_name"], "new": vname, "changelog": changelog, "added": added, "removed": removed, "count": new.len()}))
}

/// Ставит новую версию сборки поверх профиля. Сначала сохраняет точку восстановления.
pub fn update_apply(profile_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    let p = mc::find_profile(profile_id)?;
    let o = pack_origin(profile_id).ok_or("этот профиль не из сборки")?;
    let (vid, vname, url, sha1, _) = latest_for(&o, &p)?.ok_or("новой версии нет")?;
    cb("Сохраняю точку восстановления", 0.02, "");
    crate::store::snapshot(profile_id, &format!("перед обновлением сборки до {vname}"))?;
    let cf = o["provider"] == "curseforge";
    let tmp = data_dir().join("cache").join(format!("pack-upd-{}.zip", mc::slug(profile_id)));
    if !tmp.exists() {
        download(&url, &tmp, sha1.as_deref())?;
    }
    // убираем файлы старой версии сборки (свои моды игрока не трогаем)
    let dir = mc::inst_dir(profile_id);
    let new: Vec<String> = pack_files_of(&tmp, cf);
    for f in o["files"].as_array().cloned().unwrap_or_default() {
        let Some(f) = f.as_str() else { continue };
        if (f.starts_with("mods/") || f.starts_with("resourcepacks/") || f.starts_with("shaderpacks/")) && !new.iter().any(|n| n == f) {
            if let Some(rel) = safe_rel(f) {
                let _ = fs::remove_file(dir.join(&rel));
            }
        }
    }
    let mut origin = o.clone();
    origin["version"] = json!(vid);
    origin["version_name"] = json!(vname);
    let r = if cf { install_cf_file(&tmp, Some(profile_id), Some(origin), cb) } else { install_mrpack(&tmp, Some(profile_id), Some(origin), cb) };
    let _ = fs::remove_file(&tmp);
    r
}
