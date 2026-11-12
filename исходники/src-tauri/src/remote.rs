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

pub fn news() -> R<Value> {
    call("GET", "/news", None, Body::None)
}

pub fn news_save(list: Value) -> R<Value> {
    let tok = load_settings().admin_token;
    if tok.trim().is_empty() {
        return Err("Нужен токен администратора (Настройки → Администратор)".into());
    }
    call("PUT", "/news", Some(tok.trim()), Body::Json(list))
}

pub fn horizon_feed() -> R<Value> {
    const REPO: &str = "https://github.com/gerdaroot/Horizon-Offline-Launcher";
    // A new Noxel build can be tested before its server feed is deployed. Keep a short local
    // cache so GitHub's unauthenticated API is not queried on every visit to the Rust page.
    let cache_path = data_dir().join("cache/horizon-offline-feed-v2.json");
    if let Some(cached) = read_json::<Value>(&cache_path) {
        let age = now().saturating_sub(cached["fetched_at"].as_u64().unwrap_or(0));
        if age < 15 * 60 && cached["schema"].as_u64() == Some(2) && cached["feed"]["repo"].as_str() == Some(REPO) {
            return Ok(normalize_horizon_feed(cached["feed"].clone()));
        }
    }
    let releases = get_json("https://api.github.com/repos/gerdaroot/Horizon-Offline-Launcher/releases?per_page=40")?;
    let releases = releases.as_array().ok_or("GitHub вернул неверный список релизов Horizon Offline")?;
    let mut launcher: Option<Value> = None;
    let mut builds = Vec::new();
    for release in releases {
        if release["draft"].as_bool().unwrap_or(false) { continue; }
        let tag = release["tag_name"].as_str().unwrap_or("");
        if tag.starts_with("launcher@") && !release["prerelease"].as_bool().unwrap_or(false) && launcher.is_none() {
            let mut assets = serde_json::Map::new();
            if let Some(list) = release["assets"].as_array() {
                for asset in list {
                    let name = asset["name"].as_str().unwrap_or("");
                    let platform = match name {
                        "horizon-launcher-linux-x86_64.tar.gz" => Some("linux-x86_64"),
                        "horizon-launcher-windows-x64.zip" => Some("windows-x64"),
                        _ => None,
                    };
                    let digest = asset["digest"].as_str().unwrap_or("");
                    let Some(platform) = platform else { continue };
                    let Some(sha256) = digest.strip_prefix("sha256:") else { continue };
                    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) { continue; }
                    assets.insert(platform.to_owned(), json!({
                        "name": name, "url": asset["browser_download_url"], "sha256": sha256,
                        "size": asset["size"]
                    }));
                }
            }
            if !assets.is_empty() {
                launcher = Some(json!({
                    "version": tag.split_once('@').map(|(_, v)| v).unwrap_or(tag),
                    "date": release["published_at"].as_str().unwrap_or("").get(..10).unwrap_or(""),
                    "assets": assets
                }));
            }
        }
        let game_tag = tag == "nightly" || tag.starts_with("nightly-") || tag.starts_with('v');
        if game_tag && builds.len() < 12 {
            let has_game = release["assets"].as_array().is_some_and(|assets| assets.iter().any(|asset| {
                matches!(asset["name"].as_str().unwrap_or(""), "horizon-linux-x86_64.tar.gz" | "horizon-windows-x64.zip")
            }));
            let is_nightly = tag.starts_with("nightly");
            let signed_for_build = release["assets"].as_array().is_some_and(|assets| assets.iter().any(|asset| {
                asset["name"].as_str() == Some("SHA256SUMS.v2.sig")
            }));
            if has_game && (!is_nightly || signed_for_build) {
                builds.push(json!({
                    "tag": tag, "name": release["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(tag),
                    "nightly": is_nightly, "signed": true,
                    "date": release["published_at"].as_str().unwrap_or("").get(..10).unwrap_or(""),
                    "url": release["html_url"]
                }));
            }
        }
    }
    let launcher = launcher.ok_or("У Horizon Offline пока нет поддерживаемой сборки лаунчера")?;
    let feed = normalize_horizon_feed(json!({
        "version": now(), "launcher": launcher, "builds": builds,
        "site": "https://playhorizon.dev", "repo": REPO
    }));
    write_json(&cache_path, &json!({"schema": 2, "fetched_at": now(), "feed": feed}));
    Ok(feed)
}

/// Horizon Offline Launcher installs a Nightly under a version derived from its release title,
/// while the GitHub API calls that release `nightly`. Pass the installable version tag to its CLI.
fn normalize_horizon_feed(mut feed: Value) -> Value {
    if let Some(builds) = feed["builds"].as_array_mut() {
        for build in builds {
            let tag = build["tag"].as_str().unwrap_or("");
            if !tag.starts_with("nightly") { continue; }
            let name = build["name"].as_str().unwrap_or("");
            let mut words = name.strip_prefix("Nightly ").unwrap_or("").split(" · ");
            let date = words.next().filter(|d| {
                d.len() == 10 && d.as_bytes().iter().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 { *b == b'-' } else { b.is_ascii_digit() }
                })
            }).or_else(|| build["date"].as_str().filter(|d| d.len() == 10));
            if let Some(date) = date {
                let commit = words.next().filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_hexdigit()));
                let version = commit.map_or_else(|| format!("nightly-{date}"), |c| format!("nightly-{date}-{c}"));
                build["tag"] = json!(version);
            }
        }
    }
    feed
}

/// Список с сервера плюс встроенный раздел «Визуалы» (свои моды, которые лежат прямо в лаунчере).
pub fn reco() -> R<Value> {
    let mut v = call("GET", "/recommended", None, Body::None).unwrap_or_else(|_| serde_json::json!({"version": 0, "categories": []}));
    let vis = crate::reco::builtin_category();
    if let Some(cats) = v["categories"].as_array_mut() {
        if let Some(c) = cats.iter_mut().find(|c| c["id"] == "visuals") {
            // на сервере уже есть раздел: добавляем встроенные записи, которых там нет
            let ids: Vec<Value> = c["items"].as_array().map(|a| a.iter().map(|i| i["id"].clone()).collect()).unwrap_or_default();
            if let (Some(items), Some(add)) = (c["items"].as_array_mut(), vis["items"].as_array()) {
                for (n, it) in add.iter().enumerate() {
                    if !ids.contains(&it["id"]) {
                        items.insert(n, it.clone());
                    }
                }
            }
        } else {
            cats.insert(0, vis);
        }
    }
    Ok(v)
}

pub fn reco_save(list: Value) -> R<Value> {
    let tok = load_settings().admin_token;
    if tok.trim().is_empty() {
        return Err("Нужен токен администратора (Настройки → Администратор)".into());
    }
    call("PUT", "/recommended", Some(tok.trim()), Body::Json(list))
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
    let file = m["files"][PLATFORM].clone();
    // у каждой системы своя версия (выпуск только для Linux не трогает Windows); старый манифест хранит одну общую
    let latest = file["version"].as_str().or(m["version"].as_str()).unwrap_or("").to_string();
    // во Flatpak лаунчер обновляет сам Flatpak (файл программы там только для чтения)
    let newer = std::env::var_os("FLATPAK_ID").is_none() && !latest.is_empty() && !file.is_null() && ver_tuple(&latest) > ver_tuple(VERSION);
    Ok(json!({"current": VERSION, "latest": latest, "newer": newer, "notes": m["notes"], "news": m["news"], "size": file["size"]}))
}

/// Скачивает сборку из манифеста и проверяет контрольную сумму и подпись автора. Возвращает версию.
pub fn fetch_verified(dest: &std::path::Path) -> R<String> {
    let m = call("GET", "/launcher", None, Body::None)?;
    let f = &m["files"][PLATFORM];
    let version = f["version"].as_str().or(m["version"].as_str()).ok_or("в манифесте нет версии")?.to_string();
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
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Err("Noxel установлен через Flatpak: обнови его командой flatpak update".into());
    }
    let exe = exe_path()?;
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

/// Путь к запущенному файлу. После самообновления Linux помечает его как «(deleted)»: отбрасываем пометку, иначе перезапуск не найдёт файл.
fn exe_path() -> R<std::path::PathBuf> {
    let p = std::env::current_exe().map_err(e)?;
    Ok(match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(t) => std::path::PathBuf::from(t),
        None => p,
    })
}

pub fn restart() -> R<()> {
    let exe = exe_path()?;
    cmd(exe).spawn().map_err(e)?;
    Ok(())
}
