//! Друзья, присутствие и облачные миры на сервере Noxel (нужен аккаунт Noxel).
use crate::skins::{call, Body, API};
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Write, time::Duration};

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn friends(token: &str) -> R<Value> {
    call("GET", "/friends", Some(token), Body::None)
}

pub fn friend_action(token: &str, action: &str, name: &str) -> R<Value> {
    let path = match action {
        "add" => "/friends",
        "accept" => "/friends/accept",
        "remove" => "/friends/remove",
        _ => return Err("неизвестное действие".into()),
    };
    call("POST", path, Some(token), Body::Json(json!({"name": name})))
}

pub fn presence(token: &str, game: Option<String>, mc: Option<String>) -> R<Value> {
    // порты для голосовых модов объявляем, только пока мир открыт для друзей
    let ports = if crate::tunnel::host_port().is_some() { crate::tunnel::room_ports() } else { vec![] };
    call("POST", "/presence", Some(token), Body::Json(json!({"game": game, "mc": mc, "ports": ports})))
}

/// Публикует для друзей список модов профиля хозяина: друг сможет поставить такие же.
pub fn pack_publish(token: &str, profile_id: &str) -> R<Value> {
    let p = crate::mc::load_profiles().into_iter().find(|p| p.id == profile_id).ok_or("профиль не найден")?;
    let mods = crate::mods::pack_manifest(profile_id);
    call("POST", "/pack", Some(token), Body::Json(json!({"mc": p.mc, "loader": p.loader, "loader_ver": p.loader_ver, "name": p.name, "mods": mods})))
}

pub fn pack_get(token: &str, host: &str) -> R<Value> {
    call("GET", &format!("/pack/{host}"), Some(token), Body::None)
}

pub fn cloud_list(token: &str) -> R<Value> {
    call("GET", "/worlds", Some(token), Body::None)
}

fn long_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(300)).timeout_write(Duration::from_secs(900)).build()
}

/// Упаковывает мир в zip и отправляет в облако (потоком, без загрузки целиком в память).
pub fn cloud_upload(token: &str, profile_id: &str, mc: &str, world: &str) -> R<Value> {
    let b = crate::worlds::backup(profile_id, world)?;
    let path = data_dir().join("backups").join(crate::mc::slug(profile_id)).join(b["file"].as_str().ok_or("нет файла копии")?);
    let size = fs::metadata(&path).map_err(e)?.len();
    let f = fs::File::open(&path).map_err(e)?;
    let res = long_agent()
        .post(&format!("{API}/world?name={}&mc={}", enc(world), enc(mc)))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/zip")
        .set("Content-Length", &size.to_string())
        .send(f);
    match res {
        Ok(r) => r.into_json().map_err(e),
        Err(ureq::Error::Status(c, r)) => Err(r.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| format!("сервер ответил кодом {c}"))),
        Err(x) => Err(format!("не удалось отправить мир: {x}")),
    }
}

/// Скачивает мир из облака и ставит в профиль (существующий мир с таким именем переименовывается рядом).
pub fn cloud_download(token: &str, profile_id: &str, id: u64, world: &str) -> R<Value> {
    let tmp = data_dir().join("cache").join(format!("cloud-{id}.zip"));
    fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
    let r = long_agent().get(&format!("{API}/world/{id}")).set("Authorization", &format!("Bearer {token}")).call().map_err(|x| match x {
        ureq::Error::Status(404, _) => "мира нет в облаке".to_string(),
        x => format!("не удалось скачать мир: {x}"),
    })?;
    let mut out = fs::File::create(&tmp).map_err(e)?;
    std::io::copy(&mut r.into_reader(), &mut out).map_err(e)?;
    out.flush().map_err(e)?;
    let res = crate::worlds::restore_from(profile_id, &tmp, world);
    let _ = fs::remove_file(&tmp);
    res
}

pub fn cloud_delete(token: &str, id: u64) -> R<Value> {
    call("DELETE", &format!("/world/{id}"), Some(token), Body::None)
}

pub fn cloud_share(token: &str, id: u64) -> R<Value> {
    call("POST", &format!("/world/{id}/share"), Some(token), Body::None)
}

pub fn cloud_unshare(token: &str, id: u64) -> R<Value> {
    call("DELETE", &format!("/world/{id}/share"), Some(token), Body::None)
}

/// Из ссылки вида https://mr1s.site/w/<код> (или просто кода) достаёт код.
fn share_token(link: &str) -> R<String> {
    let t = link.trim().trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
    if t.len() < 10 || t.len() > 40 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("это не ссылка на мир Noxel".into());
    }
    Ok(t)
}

pub fn share_info(link: &str) -> R<Value> {
    call("GET", &format!("/share/{}", share_token(link)?), None, Body::None)
}

/// Скачивает мир по общей ссылке и ставит его в профиль.
pub fn share_import(profile_id: &str, link: &str, name: &str) -> R<Value> {
    let t = share_token(link)?;
    let tmp = data_dir().join("cache").join(format!("share-{t}.zip"));
    fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
    let r = long_agent().get(&format!("{API}/share/{t}/download")).call().map_err(|x| match x {
        ureq::Error::Status(404, _) => "ссылка недействительна: автор удалил мир или отозвал её".to_string(),
        ureq::Error::Status(429, _) => "слишком много скачиваний, подожди минуту".to_string(),
        x => format!("не удалось скачать мир: {x}"),
    })?;
    let mut out = fs::File::create(&tmp).map_err(e)?;
    std::io::copy(&mut r.into_reader(), &mut out).map_err(e)?;
    out.flush().map_err(e)?;
    let res = crate::worlds::restore_from(profile_id, &tmp, name);
    let _ = fs::remove_file(&tmp);
    res
}

// ── сборка по коду: любой (даже без друзей и аккаунта) ставит точно такую же сборку ──

/// Собирает описание профиля: точные версии модов с Modrinth, моды CurseForge, настройки модов (папка config).
pub fn code_create(token: Option<&str>, profile_id: &str) -> R<Value> {
    use base64::Engine;
    let p = crate::mc::find_profile(profile_id)?;
    let dir = crate::mc::inst_dir(&p.id);
    let idx: std::collections::HashMap<String, Value> = read_json(&dir.join(".noxel/index.json")).unwrap_or_default();
    let mut files: Vec<(String, String, String)> = vec![]; // (sha1, имя файла, тип)
    for (k, kind) in [("mods", "mod"), ("resourcepacks", "resourcepack"), ("shaderpacks", "shader")] {
        let Ok(rd) = fs::read_dir(dir.join(k)) else { continue };
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            let l = n.to_lowercase();
            if f.path().is_file() && (l.ends_with(".jar") || l.ends_with(".zip")) {
                if let Ok(h) = crate::store::sha1_file(&f.path()) {
                    files.push((h, n, kind.to_string()));
                }
            }
        }
    }
    let mut found: std::collections::HashMap<String, Value> = Default::default();
    for chunk in files.chunks(400) {
        let hs: Vec<&str> = chunk.iter().map(|x| x.0.as_str()).collect();
        if let Ok(r) = agent().post("https://api.modrinth.com/v2/version_files").send_json(json!({"hashes": hs, "algorithm": "sha1"})) {
            if let Ok(Value::Object(o)) = r.into_json::<Value>() {
                found.extend(o);
            }
        }
    }
    let (mut items, mut missing) = (vec![], vec![]);
    for (h, n, kind) in &files {
        let meta = idx.get(n);
        let title = meta.and_then(|m| m["title"].as_str()).unwrap_or(n).to_string();
        if let Some(v) = found.get(h) {
            items.push(json!({"provider": "modrinth", "id": v["project_id"], "version": v["id"], "kind": kind, "name": title}));
        } else if let Some(m) = meta.filter(|m| m["provider"] == "curseforge") {
            items.push(json!({"provider": "curseforge", "id": m["project_id"], "kind": kind, "name": title}));
        } else {
            missing.push(n.clone());
        }
    }
    // настройки модов, без личных файлов (options.txt с клавишами и серверами не берём)
    let mut cfg = vec![];
    if dir.join("config").is_dir() {
        let tmp = data_dir().join("cache").join(format!("code-{}.zip", p.id));
        fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
        {
            let mut zw = zip::ZipWriter::new(fs::File::create(&tmp).map_err(e)?);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            fn walk(zw: &mut zip::ZipWriter<fs::File>, root: &std::path::Path, cur: &std::path::Path, o: zip::write::SimpleFileOptions) -> R<()> {
                for f in fs::read_dir(cur).map_err(e)?.flatten() {
                    let p = f.path();
                    if p.is_dir() {
                        walk(zw, root, &p, o)?;
                    } else if p.is_file() && f.metadata().map(|m| m.len() < 2_000_000).unwrap_or(false) {
                        zw.start_file(p.strip_prefix(root).map_err(e)?.to_string_lossy().replace('\\', "/"), o).map_err(e)?;
                        std::io::copy(&mut fs::File::open(&p).map_err(e)?, zw).map_err(e)?;
                    }
                }
                Ok(())
            }
            walk(&mut zw, &dir, &dir.join("config"), opts)?;
            zw.finish().map_err(e)?;
        }
        cfg = fs::read(&tmp).map_err(e)?;
        let _ = fs::remove_file(&tmp);
        if cfg.len() > 3_000_000 {
            cfg.clear(); // слишком большие настройки не передаём, моды важнее
        }
    }
    let lv = p.loader_ver.clone().or_else(|| p.version_id.clone().and_then(|v| v.strip_prefix("fabric-loader-").and_then(|r| r.rsplit_once('-')).map(|x| x.0.to_string())));
    let body = json!({"name": p.name, "mc": p.mc, "loader": p.loader, "loader_ver": lv, "items": items,
        "config": if cfg.is_empty() { Value::Null } else { json!(base64::engine::general_purpose::STANDARD.encode(&cfg)) }});
    let mut r = call("POST", "/packs", token, Body::Json(body))?;
    r["missing"] = json!(missing);
    r["count"] = json!(items.len());
    Ok(r)
}

fn code_of(s: &str) -> R<String> {
    let t: String = s.trim().trim_end_matches('/').rsplit('/').next().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    if !(6..=12).contains(&t.len()) {
        return Err("это не код сборки Noxel".into());
    }
    Ok(t)
}

pub fn code_info(code: &str) -> R<Value> {
    call("GET", &format!("/packs/{}", code_of(code)?), None, Body::None)
}

/// Ставит сборку по коду отдельным профилем: те же версии игры, загрузчика и модов, плюс настройки.
pub fn code_install(code: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<Value> {
    use base64::Engine;
    let d = code_info(code)?;
    let (mcv, loader) = (d["mc"].as_str().unwrap_or(""), d["loader"].as_str().unwrap_or("vanilla"));
    let lv = d["loader_ver"].as_str().filter(|s| !s.is_empty()).map(String::from);
    let name = d["name"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("Сборка друга");
    let p = crate::mc::create_profile(mcv, loader, lv, name, None)?;
    let items = d["items"].as_array().cloned().unwrap_or_default();
    let (mut ok, mut skipped) = (0, vec![]);
    for (i, it) in items.iter().enumerate() {
        let (prov, id, kind) = (it["provider"].as_str().unwrap_or(""), it["id"].as_str().unwrap_or(""), it["kind"].as_str().unwrap_or("mod"));
        let title = it["name"].as_str().unwrap_or(id);
        cb("Ставлю моды", i as f64 / items.len().max(1) as f64, title);
        if crate::mods::already(&p.id, kind, id) {
            continue;
        }
        let ver = it["version"].as_str().map(String::from);
        match crate::mods::install(prov, kind, id, &p.id, ver, cb) {
            Ok(_) => ok += 1,
            Err(x) => skipped.push(format!("{title}: {x}")),
        }
    }
    if let Some(b64) = d["config"].as_str() {
        cb("Ставлю настройки модов", 0.97, "");
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            let tmp = data_dir().join("cache").join(format!("code-in-{}.zip", p.id));
            if fs::write(&tmp, &bytes).is_ok() {
                if let Ok(mut z) = zip::ZipArchive::new(fs::File::open(&tmp).map_err(e)?) {
                    let dir = crate::mc::inst_dir(&p.id);
                    for i in 0..z.len() {
                        let Ok(mut ent) = z.by_index(i) else { continue };
                        let Some(rel) = ent.enclosed_name().map(|x| x.to_string_lossy().replace('\\', "/")) else { continue };
                        if !rel.starts_with("config/") || ent.is_dir() {
                            continue;
                        }
                        let out = dir.join(&rel);
                        let _ = fs::create_dir_all(out.parent().unwrap());
                        if let Ok(mut f) = fs::File::create(&out) {
                            let _ = std::io::copy(&mut ent, &mut f);
                        }
                    }
                }
                let _ = fs::remove_file(&tmp);
            }
        }
    }
    cb("Готово", 1.0, "");
    Ok(json!({"profile": p.id, "name": p.name, "mc": p.mc, "loader": p.loader, "installed": ok, "skipped": skipped}))
}
