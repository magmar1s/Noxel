//! Данные с сервера Noxel: рекомендуемые моды для серверов и обновления самого лаунчера.
use crate::skins::{call, Body};
use crate::util::*;
use serde_json::{json, Value};
use std::fs;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Публичный ключ подписи релизов (Ed25519). Закрытый ключ хранится только у автора.
const RELEASE_KEY_HEX: &str = "9a2d7e4ef870ac1d75a8aed98e76c8c6588c7051ca10a13d71f62f0b0bb127b8";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn ver_tuple(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).map(|x| x.parse().unwrap_or(0)).collect()
}

pub fn servers() -> R<Value> {
    call("GET", "/servers", None, Body::None)
}

pub fn servers_save(list: Value) -> R<Value> {
    let tok = load_settings().admin_token;
    if tok.trim().is_empty() {
        return Err("Нужен токен администратора (Настройки → Администратор)".into());
    }
    call("PUT", "/servers", Some(tok.trim()), Body::Json(list))
}

/// Есть ли новая версия лаунчера и что нового.
pub fn check() -> R<Value> {
    let m = call("GET", "/launcher", None, Body::None)?;
    let latest = m["version"].as_str().unwrap_or("").to_string();
    let file = m["files"][PLATFORM].clone();
    let newer = !latest.is_empty() && !file.is_null() && ver_tuple(&latest) > ver_tuple(VERSION);
    Ok(json!({"current": VERSION, "latest": latest, "newer": newer, "notes": m["notes"], "news": m["news"], "size": file["size"]}))
}

/// Скачивает сборку из манифеста и проверяет контрольную сумму и подпись автора. Возвращает версию.
pub fn fetch_verified(dest: &std::path::Path) -> R<String> {
    let m = call("GET", "/launcher", None, Body::None)?;
    let version = m["version"].as_str().ok_or("в манифесте нет версии")?.to_string();
    let f = &m["files"][PLATFORM];
    let (url, sha, sig) = (f["url"].as_str().ok_or("в манифесте нет ссылки")?, f["sha256"].as_str().unwrap_or(""), f["sig"].as_str().unwrap_or(""));
    if !url.starts_with("https://") {
        return Err("ссылка на обновление должна быть https".into());
    }
    download(url, dest, None)?;
    let bytes = fs::read(dest).map_err(e)?;
    let bad = |m: &str| {
        let _ = fs::remove_file(dest);
        Err(m.to_string())
    };
    if crate::skins::sha256(&bytes) != sha.to_lowercase() {
        return bad("контрольная сумма обновления не совпала");
    }
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, unhex(RELEASE_KEY_HEX));
    if key.verify(&bytes, &unhex(sig)).is_err() {
        return bad("подпись обновления неверна: файл не от автора Noxel");
    }
    Ok(version)
}

/// Скачивает новую сборку, проверяет её и подменяет файл лаунчера.
pub fn update() -> R<String> {
    let exe = std::env::current_exe().map_err(e)?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Err("Это сборка для разработки: самообновление отключено".into());
    }
    let new = exe.with_extension("new");
    let version = fetch_verified(&new)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&new, fs::Permissions::from_mode(0o755));
    }
    #[cfg(windows)]
    {
        // запущенный exe нельзя перезаписать, но можно переименовать: старый файл удалится при следующем запуске
        let old = exe.with_extension("old");
        let _ = fs::remove_file(&old);
        fs::rename(&exe, &old).map_err(|x| format!("не удалось заменить файл лаунчера: {x}"))?;
    }
    fs::rename(&new, &exe).map_err(|x| format!("не удалось заменить файл лаунчера: {x}"))?;
    Ok(version)
}

pub fn restart() -> R<()> {
    let exe = std::env::current_exe().map_err(e)?;
    cmd(exe).spawn().map_err(e)?;
    Ok(())
}
