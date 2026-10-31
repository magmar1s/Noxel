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
    call("POST", "/presence", Some(token), Body::Json(json!({"game": game, "mc": mc})))
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
