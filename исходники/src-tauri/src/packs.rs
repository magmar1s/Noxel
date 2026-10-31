//! Сборки модов (modpack) с Modrinth: поиск и установка как новый профиль (формат .mrpack).
use crate::mc;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

const MR: &str = "https://api.modrinth.com/v2";

const CF: &str = "https://mr1s.site/noxel/v1/cf";

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

fn safe_rel(p: &str) -> Option<std::path::PathBuf> {
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

fn extract_overrides(z: &mut zip::ZipArchive<fs::File>, prefixes: &[String], dir: &Path) -> R<()> {
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(enc) = ent.enclosed_name() else { continue };
        let s = enc.to_string_lossy().replace('\\', "/");
        let Some(rel) = prefixes.iter().find_map(|p| s.strip_prefix(p.as_str())).filter(|r| !r.is_empty()) else { continue };
        let Some(rel) = safe_rel(rel) else { continue };
        let out = dir.join(rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        let mut f = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut f).map_err(e)?;
    }
    Ok(())
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
    let mut z = zip::ZipArchive::new(fs::File::open(&tmp).map_err(e)?).map_err(e)?;
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
    let profile = mc::create_profile(mcv, &loader, Some(lver), name, None)?;
    let dir = mc::inst_dir(&profile.id);

    // описания файлов (имя, ссылка, хеш) запрашиваем пачками через сервер Noxel
    let ids: Vec<u64> = man["files"].as_array().cloned().unwrap_or_default().iter().filter(|x| x["required"] != false).filter_map(|x| x["fileID"].as_u64()).collect();
    let mut jobs = vec![];
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
            jobs.push(Job { url: u, path: dir.join(sub).join(fname), sha1 });
        }
    }
    let total = jobs.len().max(1) as f64;
    cb("Скачиваю файлы сборки", 0.15, "");
    let failed = fetch_many(jobs, 8, &|d, _| cb("Скачиваю файлы сборки", 0.15 + 0.8 * d as f64 / total, &format!("{d} из {}", total as u64)));
    cb("Распаковываю настройки", 0.97, "");
    let ov = man["overrides"].as_str().unwrap_or("overrides").trim_matches('/').to_string();
    extract_overrides(&mut z, &[format!("{ov}/")], &dir)?;
    let _ = fs::remove_file(&tmp);
    cb("Готово", 1.0, "");
    Ok(json!({"profile": profile.id, "name": profile.name, "mc": profile.mc, "loader": profile.loader, "failed": failed}))
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
