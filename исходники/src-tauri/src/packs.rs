//! Сборки модов (modpack) с Modrinth: поиск и установка как новый профиль (формат .mrpack).
use crate::mc;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

const MR: &str = "https://api.modrinth.com/v2";

pub fn search(query: &str, sort: &str, offset: u32) -> R<Value> {
    let facets = serde_json::to_string(&vec![vec!["project_type:modpack".to_string()]]).map_err(e)?;
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
    let hits: Vec<Value> = r["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let loaders: Vec<&str> = h["categories"].as_array().map(|c| c.iter().filter_map(|x| x.as_str()).filter(|x| ["fabric", "quilt", "forge", "neoforge"].contains(x)).collect()).unwrap_or_default();
            json!({
                "id": h["project_id"], "slug": h["slug"], "title": h["title"], "desc": h["description"], "author": h["author"],
                "icon": h["icon_url"], "downloads": h["downloads"], "mc": h["versions"].as_array().and_then(|v| v.last()).cloned().unwrap_or(Value::Null),
                "loaders": loaders, "url": format!("https://modrinth.com/modpack/{}", h["slug"].as_str().unwrap_or("")),
            })
        })
        .collect();
    Ok(json!({"hits": hits, "total": r["total_hits"]}))
}

fn safe_rel(p: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() || p.contains("..") || p.contains('\0') || p.starts_with('/') || p.starts_with('\\') {
        return None;
    }
    Some(path.to_path_buf())
}

/// Скачивает сборку и создаёт под неё профиль. Игра и загрузчик доустановятся при первом запуске.
pub fn install(project_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    cb("Ищу версию сборки", 0.0, "");
    let vs: Value = agent().get(&format!("{MR}/project/{project_id}/version")).call().map_err(e)?.into_json().map_err(e)?;
    let list = vs.as_array().ok_or("у сборки нет версий")?;
    let v = list.iter().find(|x| x["version_type"] == "release").or(list.first()).ok_or("у сборки нет версий")?;
    let file = v["files"].as_array().and_then(|f| f.iter().find(|x| x["primary"] == true).or(f.first())).ok_or("в версии сборки нет файла")?;
    let url = file["url"].as_str().ok_or("нет ссылки на сборку")?;
    let tmp = data_dir().join("cache").join(format!("pack-{project_id}.mrpack"));
    cb("Скачиваю сборку", 0.05, "");
    download(url, &tmp, file["hashes"]["sha1"].as_str())?;

    let mut z = zip::ZipArchive::new(fs::File::open(&tmp).map_err(e)?).map_err(e)?;
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
    let profile = mc::create_profile(mcv, &loader, Some(lver), name, None)?;
    let dir = mc::inst_dir(&profile.id);

    let mut jobs = vec![];
    for f in idx["files"].as_array().cloned().unwrap_or_default() {
        if f["env"]["client"] == "unsupported" {
            continue;
        }
        let Some(rel) = f["path"].as_str().and_then(safe_rel) else { continue };
        let Some(u) = f["downloads"][0].as_str() else { continue };
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
        let out = dir.join(rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        let mut f = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut f).map_err(e)?;
    }
    let _ = fs::remove_file(&tmp);
    cb("Готово", 1.0, "");
    Ok(json!({"profile": profile.id, "name": profile.name, "mc": profile.mc, "loader": profile.loader}))
}
