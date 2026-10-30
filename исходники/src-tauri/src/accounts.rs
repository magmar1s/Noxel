use crate::mc::offline_uuid;
use crate::skins::{self, Body};
use crate::stats;
use crate::util::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Адрес, по которому сервер Noxel раздаёт скины и плащи (его принимает `/skin url` в SkinsRestorer).
pub const FILES_URL: &str = "https://mr1s.site/noxel/f";

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SkinEntry {
    pub id: u64,
    pub hash: String,
    pub model: String,
    pub created: u64,
    #[serde(default)]
    pub remote_id: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Account {
    pub id: String,
    pub kind: String, // "local" | "microsoft" | "noxel"
    pub name: String,
    pub uuid: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires: u64,
    #[serde(default)]
    pub xuid: String,
    /// Токен сервера Noxel (пароль нигде не хранится).
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub skins: Vec<SkinEntry>,
    #[serde(default)]
    pub skin_cur: Option<u64>,
    #[serde(default)]
    pub cape: Option<String>,
    /// Секрет для паролей на серверах (автовход). Не покидает лаунчер, у аккаунта Noxel хранится и в облаке.
    #[serde(default)]
    pub server_secret: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
    pub selected: String,
    pub list: Vec<Account>,
}

fn path() -> PathBuf {
    data_dir().join("accounts.json")
}

pub fn load() -> Store {
    let mut s: Store = read_json(&path()).unwrap_or_default();
    if s.list.is_empty() {
        // первый запуск: берём ник из старых настроек
        let nick = load_settings().nickname;
        let name = if valid_name(&nick) { nick } else { "Player".into() };
        s.list.push(local(&name));
        s.selected = s.list[0].id.clone();
        save(&s);
    }
    if !s.list.iter().any(|a| a.id == s.selected) {
        s.selected = s.list[0].id.clone();
    }
    s
}

pub fn save(s: &Store) {
    write_json(&path(), s);
}

fn valid_name(n: &str) -> bool {
    (3..=16).contains(&n.chars().count()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn local(name: &str) -> Account {
    Account { id: format!("local-{}", name.to_lowercase()), kind: "local".into(), name: name.into(), uuid: offline_uuid(name), created: now(), ..Default::default() }
}

pub fn get(id: &str) -> R<Account> {
    load().list.into_iter().find(|a| a.id == id).ok_or_else(|| "Аккаунт не найден".to_string())
}

fn put(a: Account) {
    let mut s = load();
    match s.list.iter_mut().find(|x| x.id == a.id) {
        Some(x) => *x = a,
        None => s.list.push(a),
    }
    save(&s);
}

fn mutate(id: &str, f: impl FnOnce(&mut Account)) -> R<Account> {
    let mut a = get(id)?;
    f(&mut a);
    put(a.clone());
    Ok(a)
}

fn cur_skin(a: &Account) -> Option<&SkinEntry> {
    a.skin_cur.and_then(|id| a.skins.iter().find(|s| s.id == id))
}

/// Список для интерфейса: без токенов, с текущим скином (для голов в списке и рейке).
pub fn list() -> Value {
    let s = load();
    json!(s
        .list
        .iter()
        .map(|a| json!({
            "id": a.id, "kind": a.kind, "name": a.name, "uuid": a.uuid, "selected": a.id == s.selected,
            "skin": cur_skin(a).map(|k| json!({"hash": k.hash, "model": k.model})),
        }))
        .collect::<Vec<_>>())
}

pub fn add_local(name: &str) -> R<()> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("Ник: от 3 до 16 символов, латиница, цифры и _".into());
    }
    let mut s = load();
    let a = local(name);
    if s.list.iter().any(|x| x.id == a.id) {
        return Err("Такой аккаунт уже есть".into());
    }
    s.selected = a.id.clone();
    s.list.push(a);
    save(&s);
    Ok(())
}

pub fn remove(id: &str) -> R<()> {
    let mut s = load();
    if s.list.len() <= 1 {
        return Err("Нужен хотя бы один аккаунт".into());
    }
    if let Some(a) = s.list.iter().find(|a| a.id == id) {
        if a.kind == "noxel" && !a.token.is_empty() {
            let _ = skins::call("POST", "/logout", Some(&a.token), Body::None);
        }
    }
    s.list.retain(|a| a.id != id);
    save(&s);
    Ok(())
}

pub fn select(id: &str) -> R<()> {
    let mut s = load();
    if !s.list.iter().any(|a| a.id == id) {
        return Err("Аккаунт не найден".into());
    }
    s.selected = id.to_string();
    save(&s);
    Ok(())
}

/// Активный аккаунт; у лицензионного при необходимости обновляет токен.
pub fn active_fresh() -> R<Account> {
    let s = load();
    let a = s.list.iter().find(|a| a.id == s.selected).cloned().ok_or("Нет аккаунта")?;
    refresh_if_needed(a)
}

fn refresh_if_needed(mut a: Account) -> R<Account> {
    if a.kind == "microsoft" && a.expires < now() + 300 {
        let cid = client_id()?;
        let tok = post_form(
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
            &[("client_id", &cid), ("grant_type", "refresh_token"), ("refresh_token", &a.refresh_token), ("scope", "XboxLive.signin offline_access")],
        )?;
        let at = tok["access_token"].as_str().ok_or("Вход истёк. Войди в аккаунт Microsoft заново на странице «Аккаунты».")?;
        let rt = tok["refresh_token"].as_str().unwrap_or(&a.refresh_token).to_string();
        let fresh = minecraft_login(at, &rt)?;
        a.access_token = fresh.access_token;
        a.refresh_token = fresh.refresh_token;
        a.expires = fresh.expires;
        a.xuid = fresh.xuid;
        put(a.clone());
    }
    Ok(a)
}

// ── Noxel: аккаунт с паролем на сервере ──

pub fn noxel_auth(name: &str, password: &str, register: bool) -> R<()> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("Ник: от 3 до 16 символов, латиница, цифры и _".into());
    }
    let r = skins::call("POST", if register { "/register" } else { "/login" }, None, Body::Json(json!({"name": name, "password": password})))?;
    let token = r["token"].as_str().ok_or("сервер не выдал токен")?.to_string();
    let acc = &r["account"];
    let mut a = get(&format!("noxel-{}", name.to_lowercase())).unwrap_or(Account {
        id: format!("noxel-{}", name.to_lowercase()),
        kind: "noxel".into(),
        created: now(),
        ..Default::default()
    });
    a.name = acc["name"].as_str().unwrap_or(name).to_string();
    a.uuid = acc["uuid"].as_str().unwrap_or(&offline_uuid(name)).to_string();
    a.token = token;
    pull_noxel(&mut a)?;
    let mut s = load();
    s.selected = a.id.clone();
    save(&s);
    put(a);
    Ok(())
}

fn ensure_local(url: &str, hash: &str) -> R<()> {
    if skins::load(hash).is_err() {
        let b = skins::fetch_png(url)?;
        if skins::sha256(&b) == hash {
            skins::store(&b)?;
        }
    }
    Ok(())
}

/// Подтягивает историю скинов, текущий плащ и статистику с сервера Noxel.
fn pull_noxel(a: &mut Account) -> R<()> {
    let sk = skins::call("GET", "/skin", Some(&a.token), Body::None)?;
    let hist = sk["history"].as_array().cloned().unwrap_or_default();
    for it in hist.iter().rev() {
        let (Some(hash), Some(url)) = (it["hash"].as_str(), it["url"].as_str()) else { continue };
        if ensure_local(url, hash).is_err() {
            continue;
        }
        let model = it["model"].as_str().unwrap_or("classic").to_string();
        let rid = it["id"].as_u64();
        match a.skins.iter_mut().find(|e| e.hash == hash && e.model == model) {
            Some(e) => e.remote_id = rid,
            None => {
                let id = a.skins.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                a.skins.push(SkinEntry { id, hash: hash.into(), model, created: it["created"].as_u64().unwrap_or_else(now), remote_id: rid });
            }
        }
    }
    if let Some(h) = sk["current"]["hash"].as_str() {
        let m = sk["current"]["model"].as_str().unwrap_or("classic");
        a.skin_cur = a.skins.iter().find(|e| e.hash == h && e.model == m).map(|e| e.id);
    }
    let cp = skins::call("GET", "/cape", Some(&a.token), Body::None)?;
    a.cape = match (cp["current"]["hash"].as_str(), cp["current"]["url"].as_str()) {
        (Some(h), Some(u)) => ensure_local(u, h).ok().map(|_| h.to_string()),
        _ => None,
    };
    // статистика из облака объединяется с локальной
    if let Ok(d) = skins::call("GET", "/data", Some(&a.token), Body::None) {
        if a.server_secret.is_empty() {
            if let Some(s) = d["server_secret"].as_str().filter(|s| s.len() == 64) {
                a.server_secret = s.to_string();
            }
        }
        if let Ok(ss) = serde_json::from_value::<Vec<stats::Session>>(d["sessions"].clone()) {
            stats::merge(&a.id, ss);
        }
    }
    Ok(())
}

/// Отправляет статистику и сводку профилей в облако (вызывается после игры).
pub fn sync_up(id: &str) {
    let Ok(a) = get(id) else { return };
    if a.kind != "noxel" || a.token.is_empty() {
        return;
    }
    let profiles: Vec<Value> = crate::mc::load_profiles()
        .iter()
        .map(|p| {
            let mods = crate::mods::list(&p.id, "mod").ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            json!({"name": p.name, "mc": p.mc, "loader": p.loader, "mods": mods.iter().map(|m| m["title"].as_str().or(m["name"].as_str()).unwrap_or("")).collect::<Vec<_>>()})
        })
        .collect();
    let mut body = json!({"sessions": stats::sessions(id), "profiles": profiles});
    if !a.server_secret.is_empty() {
        body["server_secret"] = json!(a.server_secret);
    }
    let _ = skins::call("PUT", "/data", Some(&a.token), Body::Json(body));
}

// ── скины и плащи ──

pub fn skin_apply(id: &str, file: &str, model: Option<String>) -> R<()> {
    let bytes = std::fs::read(file).map_err(|_| "не удалось прочитать файл")?;
    if bytes.len() > 2_000_000 {
        return Err("файл слишком большой".into());
    }
    let (png, detected) = skins::normalize_skin(&bytes)?;
    let model = model.filter(|m| m == "slim" || m == "classic").unwrap_or(detected);
    upload_skin(id, &png, &model, None)
}

fn upload_skin(id: &str, png: &[u8], model: &str, reuse: Option<u64>) -> R<()> {
    let hash = skins::store(png)?;
    let a = refresh_if_needed(get(id)?)?;
    let mut remote = None;
    match a.kind.as_str() {
        "noxel" => {
            let r = skins::call("POST", &format!("/skin?model={model}"), Some(&a.token), Body::Bytes(png.to_vec()))?;
            remote = r["skin"]["id"].as_u64();
        }
        "microsoft" => mojang_upload(&a.access_token, png, model)?,
        _ => {}
    }
    mutate(id, |a| {
        let eid = match reuse.filter(|r| a.skins.iter().any(|e| e.id == *r)) {
            Some(r) => r,
            None => match a.skins.iter().find(|e| e.hash == hash && e.model == model) {
                Some(e) => e.id,
                None => {
                    let n = a.skins.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                    a.skins.push(SkinEntry { id: n, hash: hash.clone(), model: model.into(), created: now(), remote_id: None });
                    n
                }
            },
        };
        if let Some(e) = a.skins.iter_mut().find(|e| e.id == eid) {
            e.remote_id = remote.or(e.remote_id);
            e.created = now();
        }
        a.skin_cur = Some(eid);
    })?;
    Ok(())
}

pub fn skin_use(id: &str, entry: u64) -> R<()> {
    let a = get(id)?;
    let e = a.skins.iter().find(|e| e.id == entry).ok_or("скин не найден в истории")?.clone();
    let png = skins::load(&e.hash)?;
    upload_skin(id, &png, &e.model, Some(entry))
}

pub fn skin_remove(id: &str, entry: u64) -> R<()> {
    let a = get(id)?;
    if let (Some(e), true) = (a.skins.iter().find(|e| e.id == entry), a.kind == "noxel") {
        if let Some(rid) = e.remote_id {
            let _ = skins::call("DELETE", &format!("/skin/{rid}"), Some(&a.token), Body::None);
        }
    }
    mutate(id, |a| {
        a.skins.retain(|e| e.id != entry);
        if a.skin_cur == Some(entry) {
            a.skin_cur = None;
        }
    })?;
    Ok(())
}

pub fn cape_apply(id: &str, file: &str) -> R<()> {
    let a = get(id)?;
    if a.kind != "noxel" {
        return Err("Плащи хранятся на сервере Noxel: войди в аккаунт Noxel".into());
    }
    let bytes = std::fs::read(file).map_err(|_| "не удалось прочитать файл")?;
    skins::check_cape(&bytes)?;
    skins::call("POST", "/cape", Some(&a.token), Body::Bytes(bytes.clone()))?;
    let h = skins::store(&bytes)?;
    mutate(id, |a| a.cape = Some(h))?;
    Ok(())
}

pub fn cape_clear(id: &str) -> R<()> {
    let a = get(id)?;
    if a.kind == "noxel" {
        skins::call("POST", "/cape/select", Some(&a.token), Body::Json(json!({"id": null})))?;
    }
    mutate(id, |a| a.cape = None)?;
    Ok(())
}

/// Секрет аккаунта для автовхода; создаётся при первом использовании.
pub fn ensure_secret(id: &str) -> R<String> {
    use ring::rand::SecureRandom;
    let a = get(id)?;
    if !a.server_secret.is_empty() {
        return Ok(a.server_secret);
    }
    let mut b = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut b).map_err(|_| "нет источника случайности")?;
    let s: String = b.iter().map(|x| format!("{x:02x}")).collect();
    mutate(id, |a| a.server_secret = s.clone())?;
    Ok(s)
}

/// Команда для SkinsRestorer с постоянной ссылкой на скин (только у аккаунтов Noxel с загруженным скином).
pub fn skin_command(id: &str) -> Option<(String, String)> {
    let a = get(id).ok()?;
    if a.kind != "noxel" {
        return None;
    }
    let e = cur_skin(&a)?.clone();
    // SkinsRestorer скачивает скин по ссылке: файл обязан лежать на сервере, иначе игрок получит «недействительный URL»
    let url = format!("{FILES_URL}/{}.png", e.hash);
    let online = |u: &str| agent().head(u).call().is_ok();
    if !online(&url) {
        if let Ok(png) = skins::load(&e.hash) {
            let _ = upload_skin(id, &png, &e.model, Some(e.id));
        }
        if !online(&url) {
            return None;
        }
    }
    Some((e.hash.clone(), format!("/skin url {url}{}", if e.model == "slim" { " slim" } else { "" })))
}

/// Для лицензионного аккаунта берём скин и плащ прямо с серверов Mojang.
fn pull_ms(a: &mut Account) -> R<()> {
    let prof: Value = agent()
        .get("https://api.minecraftservices.com/minecraft/profile")
        .set("Authorization", &format!("Bearer {}", a.access_token))
        .call()
        .map_err(e)?
        .into_json()
        .map_err(e)?;
    let active = |k: &str| prof[k].as_array().and_then(|l| l.iter().find(|x| x["state"] == "ACTIVE")).cloned();
    if let Some(s) = active("skins") {
        let b = skins::fetch_png(s["url"].as_str().unwrap_or(""))?;
        let (png, detected) = skins::normalize_skin(&b)?;
        let model = if s["variant"] == "SLIM" { "slim".to_string() } else if s["variant"] == "CLASSIC" { "classic".to_string() } else { detected };
        let hash = skins::store(&png)?;
        let eid = match a.skins.iter().find(|e| e.hash == hash && e.model == model) {
            Some(e) => e.id,
            None => {
                let n = a.skins.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                a.skins.push(SkinEntry { id: n, hash, model, created: now(), remote_id: None });
                n
            }
        };
        a.skin_cur = Some(eid);
    }
    a.cape = match active("capes").and_then(|c| c["url"].as_str().map(String::from)) {
        Some(u) => skins::fetch_png(&u).ok().and_then(|b| skins::store(&b).ok()),
        None => None,
    };
    Ok(())
}

fn mojang_upload(token: &str, png: &[u8], model: &str) -> R<()> {
    let boundary = "----noxelboundary7d4a6d158c9";
    let mut body: Vec<u8> = vec![];
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{}\r\n", if model == "slim" { "slim" } else { "classic" }).as_bytes());
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend(png);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    match agent()
        .post("https://api.minecraftservices.com/minecraft/profile/skins")
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send_bytes(&body)
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(c, _)) => Err(format!("Mojang не принял скин (код {c})")),
        Err(x) => Err(x.to_string()),
    }
}

/// Всё для правой панели страницы аккаунта.
pub fn details(id: &str, refresh: bool) -> R<Value> {
    let mut a = get(id)?;
    if refresh {
        if a.kind == "microsoft" {
            if let Ok(mut f) = refresh_if_needed(a.clone()) {
                if pull_ms(&mut f).is_ok() {
                    put(f.clone());
                    a = f;
                }
            }
        } else if a.kind == "noxel" && !a.token.is_empty() {
            let mut f = a.clone();
            if pull_noxel(&mut f).is_ok() {
                put(f.clone());
                a = f;
            }
        }
    }
    let cur = cur_skin(&a).cloned();
    let mut hist: Vec<&SkinEntry> = a.skins.iter().collect();
    hist.sort_by(|x, y| y.created.cmp(&x.created));
    let history: Vec<Value> = hist
        .iter()
        .take(40)
        .map(|e| json!({"id": e.id, "hash": e.hash, "model": e.model, "created": e.created, "current": Some(e.id) == a.skin_cur}))
        .collect();
    Ok(json!({
        "id": a.id, "kind": a.kind, "name": a.name, "uuid": a.uuid, "created": a.created,
        "skin": cur.as_ref().map(|k| json!({"hash": k.hash, "model": k.model, "url": if a.kind == "noxel" { Some(format!("{FILES_URL}/{}.png", k.hash)) } else { None }})),
        "cape": a.cape,
        "history": history,
        "stats": stats::summary(&a.id),
        "worlds": stats::worlds(),
    }))
}

// ── Microsoft: вход по коду (device code) ──

static CANCEL: AtomicBool = AtomicBool::new(false);

fn client_id() -> R<String> {
    let c = load_settings().ms_client_id;
    if c.trim().is_empty() {
        Err("Для входа через Microsoft нужен Client ID приложения Azure. Вставь его в настройках (раздел «Лицензия Microsoft»).".into())
    } else {
        Ok(c.trim().to_string())
    }
}

/// POST формы; тело разбирается и при коде 4xx (Microsoft так отвечает, пока вход не завершён).
fn post_form(url: &str, f: &[(&str, &str)]) -> R<Value> {
    match agent().post(url).send_form(f) {
        Ok(r) => r.into_json().map_err(e),
        Err(ureq::Error::Status(_, r)) => r.into_json().map_err(e),
        Err(x) => Err(x.to_string()),
    }
}

fn post_json(url: &str, body: Value) -> R<Value> {
    match agent().post(url).set("Accept", "application/json").send_json(body) {
        Ok(r) => r.into_json().map_err(e),
        Err(ureq::Error::Status(c, r)) => {
            let v: Value = r.into_json().unwrap_or(json!({}));
            Err(match v["XErr"].as_u64() {
                Some(2148916233) => "У этой учётной записи Microsoft нет профиля Xbox. Создай его на xbox.com.".into(),
                Some(2148916238) => "Детский аккаунт: добавь его в семейную группу Microsoft.".into(),
                _ => format!("Сервер ответил {c}: {}", v["error"].as_str().or(v["errorMessage"].as_str()).unwrap_or("отказ")),
            })
        }
        Err(x) => Err(x.to_string()),
    }
}

pub fn ms_start() -> R<Value> {
    let cid = client_id()?;
    CANCEL.store(false, Ordering::SeqCst);
    let r = post_form(
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode",
        &[("client_id", &cid), ("scope", "XboxLive.signin offline_access")],
    )?;
    if r["device_code"].is_null() {
        return Err(format!("Microsoft отказал: {}", r["error_description"].as_str().unwrap_or("неверный Client ID")));
    }
    Ok(r)
}

pub fn ms_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

pub fn ms_wait(device_code: &str, interval: u64, expires_in: u64) -> R<()> {
    let cid = client_id()?;
    let deadline = now() + expires_in;
    let mut every = interval.max(2);
    loop {
        for _ in 0..every * 4 {
            if CANCEL.load(Ordering::SeqCst) {
                return Err("Отменено".into());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if now() > deadline {
            return Err("Время ожидания вышло. Попробуй ещё раз.".into());
        }
        let t = post_form(
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
            &[("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("client_id", &cid), ("device_code", device_code)],
        )?;
        if let Some(at) = t["access_token"].as_str() {
            let mut acc = minecraft_login(at, t["refresh_token"].as_str().unwrap_or(""))?;
            if let Ok(old) = get(&acc.id) {
                acc.skins = old.skins;
                acc.skin_cur = old.skin_cur;
                acc.created = old.created;
            }
            let _ = pull_ms(&mut acc);
            let mut s = load();
            s.selected = acc.id.clone();
            save(&s);
            put(acc);
            return Ok(());
        }
        match t["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => every += 5,
            Some(other) => return Err(format!("Вход не удался: {}", t["error_description"].as_str().unwrap_or(other))),
            None => return Err("Неожиданный ответ Microsoft".into()),
        }
    }
}

/// Цепочка Xbox Live → XSTS → Minecraft Services.
fn minecraft_login(ms_token: &str, refresh: &str) -> R<Account> {
    let xbl = post_json(
        "https://user.auth.xboxlive.com/user/authenticate",
        json!({"Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms_token}")},
               "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"}),
    )?;
    let xbl_token = xbl["Token"].as_str().ok_or("Xbox Live не выдал токен")?;
    let xsts = post_json(
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        json!({"Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl_token]},
               "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"}),
    )?;
    let xsts_token = xsts["Token"].as_str().ok_or("XSTS не выдал токен")?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"].as_str().ok_or("нет uhs")?;
    let xuid = xsts["DisplayClaims"]["xui"][0]["xid"].as_str().unwrap_or("").to_string();
    let mc = post_json("https://api.minecraftservices.com/authentication/login_with_xbox", json!({"identityToken": format!("XBL3.0 x={uhs};{xsts_token}")}))?;
    let token = mc["access_token"].as_str().ok_or("Minecraft не выдал токен")?.to_string();
    let prof = match agent().get("https://api.minecraftservices.com/minecraft/profile").set("Authorization", &format!("Bearer {token}")).call() {
        Ok(r) => r.into_json::<Value>().map_err(e)?,
        Err(ureq::Error::Status(404, _)) => return Err("На этом аккаунте нет лицензии Minecraft: Java Edition.".into()),
        Err(x) => return Err(x.to_string()),
    };
    let name = prof["name"].as_str().ok_or("нет имени профиля")?.to_string();
    let uuid = prof["id"].as_str().unwrap_or("").to_string();
    Ok(Account {
        id: format!("ms-{uuid}"),
        kind: "microsoft".into(),
        name,
        uuid,
        access_token: token,
        refresh_token: refresh.to_string(),
        expires: now() + mc["expires_in"].as_u64().unwrap_or(86400),
        xuid,
        created: now(),
        ..Default::default()
    })
}
