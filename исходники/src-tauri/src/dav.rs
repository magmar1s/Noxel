//! Своё облако по WebDAV (Яндекс Диск, Nextcloud и другие): копии миров лежат у пользователя, а не на сервере Noxel.
use crate::util::*;
use crate::worlds;
use base64::Engine;
use serde_json::{json, Value};
use std::{fs, time::Duration};

fn auth(s: &Settings) -> String {
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", s.dav_user, s.dav_pass)))
}

fn base(s: &Settings) -> R<String> {
    let u = s.dav_url.trim().trim_end_matches('/').to_string();
    if u.is_empty() || s.dav_user.is_empty() {
        return Err("Сначала укажи адрес, логин и пароль своего облака в настройках".into());
    }
    if !(u.starts_with("https://") || u.starts_with("http://127.0.0.1") || u.starts_with("http://localhost")) {
        return Err("Адрес облака должен начинаться с https://".into());
    }
    Ok(u)
}

fn client() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(300)).timeout_write(Duration::from_secs(900)).build()
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn status_err(x: ureq::Error) -> String {
    match x {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => "облако не приняло логин или пароль (для Яндекс Диска нужен пароль приложения «Файлы WebDAV»)".into(),
        ureq::Error::Status(507, _) => "в облаке нет места".into(),
        ureq::Error::Status(c, _) => format!("облако ответило кодом {c}"),
        x => format!("нет связи с облаком: {x}"),
    }
}

fn mkcol(s: &Settings, url: &str) -> R<()> {
    match client().request("MKCOL", url).set("Authorization", &auth(s)).call() {
        Ok(_) | Err(ureq::Error::Status(405, _)) | Err(ureq::Error::Status(409, _)) => Ok(()),
        Err(x) => Err(status_err(x)),
    }
}

fn ensure_dirs(s: &Settings, b: &str, profile: &str) -> R<String> {
    let root = format!("{b}/Noxel");
    mkcol(s, &root)?;
    let dir = format!("{root}/{}", enc(profile));
    mkcol(s, &dir)?;
    Ok(dir)
}

/// Проверка подключения: возвращает число папок и файлов в корне облака.
pub fn test() -> R<Value> {
    let s = load_settings();
    let b = base(&s)?;
    let r = client().request("PROPFIND", &format!("{b}/")).set("Authorization", &auth(&s)).set("Depth", "0").send_string(r#"<?xml version="1.0"?><propfind xmlns="DAV:"><prop><resourcetype/></prop></propfind>"#).map_err(status_err)?;
    Ok(json!({"ok": true, "status": r.status()}))
}

pub fn upload_world(profile: &str, world: &str) -> R<u64> {
    let s = load_settings();
    let b = base(&s)?;
    let dir = ensure_dirs(&s, &b, profile)?;
    let tmp = data_dir().join("cache").join(format!("dav-{}-{}.zip", crate::mc::slug(profile), now()));
    fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
    worlds::zip_world(profile, world, &tmp)?;
    let size = fs::metadata(&tmp).map_err(e)?.len();
    let f = fs::File::open(&tmp).map_err(e)?;
    let r = client().put(&format!("{dir}/{}.zip", enc(world))).set("Authorization", &auth(&s)).set("Content-Length", &size.to_string()).set("Content-Type", "application/zip").send(f);
    let _ = fs::remove_file(&tmp);
    r.map_err(status_err)?;
    Ok(size)
}

/// Копии миров профиля, лежащие в облаке: [{name, size}].
pub fn list(profile: &str) -> R<Value> {
    let s = load_settings();
    let b = base(&s)?;
    let dir = format!("{b}/Noxel/{}", enc(profile));
    let body = match client().request("PROPFIND", &format!("{dir}/")).set("Authorization", &auth(&s)).set("Depth", "1").send_string(r#"<?xml version="1.0"?><propfind xmlns="DAV:"><prop><getcontentlength/><getlastmodified/></prop></propfind>"#) {
        Ok(r) => r.into_string().map_err(e)?,
        Err(ureq::Error::Status(404, _)) => return Ok(json!([])),
        Err(x) => return Err(status_err(x)),
    };
    let mut out = vec![];
    let low = body.to_lowercase();
    let mut pos = 0;
    while let Some(i) = low[pos..].find("response>") {
        // блок ответа: от открывающего тега до закрывающего
        let start = pos + i + "response>".len();
        let Some(j) = low[start..].find("response>") else { break };
        let block = &body[start..start + j];
        let lb = &low[start..start + j];
        pos = start + j + "response>".len();
        let href = tag(block, lb, "href");
        let name = dec(href.trim_end_matches('/').rsplit('/').next().unwrap_or(""));
        if !name.to_lowercase().ends_with(".zip") || lb.contains("collection") {
            continue;
        }
        let size: u64 = tag(block, lb, "getcontentlength").trim().parse().unwrap_or(0);
        out.push(json!({"name": name.trim_end_matches(".zip").trim_end_matches(".ZIP"), "size": size, "modified": tag(block, lb, "getlastmodified")}));
    }
    Ok(json!(out))
}

fn tag(block: &str, low: &str, name: &str) -> String {
    let Some(i) = low.find(&format!("{name}>")) else { return String::new() };
    let start = i + name.len() + 1;
    let end = low[start..].find("</").map(|k| start + k).unwrap_or(block.len());
    block[start..end].trim().to_string()
}

pub fn download_world(profile: &str, world: &str) -> R<Value> {
    let s = load_settings();
    let b = base(&s)?;
    let tmp = data_dir().join("cache").join(format!("davdl-{}.zip", now()));
    fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
    let r = client().get(&format!("{b}/Noxel/{}/{}.zip", enc(profile), enc(world))).set("Authorization", &auth(&s)).call().map_err(status_err)?;
    std::io::copy(&mut r.into_reader(), &mut fs::File::create(&tmp).map_err(e)?).map_err(e)?;
    let res = worlds::restore_from(profile, &tmp, world);
    let _ = fs::remove_file(&tmp);
    res
}

/// Загружает в облако только те миры профиля, которые изменились с прошлой синхронизации.
pub fn sync_changed(profile: &str) -> R<u32> {
    let mut state: Value = read_json(&data_dir().join("dav_sync.json")).unwrap_or_else(|| json!({}));
    let mut n = 0;
    let saves = crate::mc::inst_dir(profile).join("saves");
    for ent in fs::read_dir(&saves).map_err(e)?.flatten() {
        let world = ent.file_name().to_string_lossy().to_string();
        if !ent.path().join("level.dat").exists() {
            continue;
        }
        let m = worlds::world_mtime(profile, &world);
        let key = format!("{profile}/{world}");
        if state[&key].as_u64() == Some(m) {
            continue;
        }
        upload_world(profile, &world)?;
        state[&key] = json!(m);
        n += 1;
    }
    write_json(&data_dir().join("dav_sync.json"), &state);
    Ok(n)
}
