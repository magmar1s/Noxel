//! Автовход на серверах: в логе игры ловим приглашения `/register` и `/login` и просим агента внутри игры
//! набрать нужную команду. Пароль у каждого сервера свой: он считается из секрета аккаунта и адреса сервера,
//! поэтому один сервер не узнаёт пароль от другого. Для SkinsRestorer после входа отправляется `/skin url ...`.
use crate::{accounts, util::*};
use base64::Engine;
use ring::hmac;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const AGENT: &[u8] = include_bytes!("../assets/noxel-agent.jar");

/// Кладёт агента в папку данных (если его там ещё нет или он обновился) и возвращает путь.
pub fn install_agent() -> R<PathBuf> {
    let p = data_dir().join("agent").join("noxel-agent.jar");
    // сравниваем содержимое, а не длину: устаревший агент не должен остаться, иначе игра падает
    if fs::read(&p).map(|b| b != AGENT).unwrap_or(true) {
        fs::create_dir_all(p.parent().unwrap()).map_err(e)?;
        fs::write(&p, AGENT).map_err(e)?;
    }
    Ok(p)
}

const ICON_256: &[u8] = include_bytes!("../assets/game-icon-256.png");
const ICON_64: &[u8] = include_bytes!("../assets/game-icon-64.png");

/// Linux: значок для панелей, которые подбирают его по классу окна (у игры класс совпадает с заголовком окна).
/// На Windows значок окна ставит сам агент через GLFW.
pub fn install_game_entry(mc: &str) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", std::env::var("HOME").unwrap_or_default()));
    let icons = PathBuf::from(&base).join("icons/hicolor");
    for (sub, bytes) in [("256x256/apps", ICON_256), ("64x64/apps", ICON_64)] {
        let d = icons.join(sub);
        if fs::create_dir_all(&d).is_ok() && fs::metadata(d.join("noxel-minecraft.png")).map(|m| m.len() != bytes.len() as u64).unwrap_or(true) {
            let _ = fs::write(d.join("noxel-minecraft.png"), bytes);
        }
    }
    let apps = PathBuf::from(&base).join("applications");
    let _ = fs::create_dir_all(&apps);
    for (suffix, class) in [("", format!("Minecraft {mc}")), ("-modded", format!("Minecraft* {mc}"))] {
        let f = apps.join(format!("noxel-game-{}{suffix}.desktop", mc.replace(' ', "_")));
        let text = format!("[Desktop Entry]\nType=Application\nName=Minecraft {mc} (Noxel)\nIcon=noxel-minecraft\nExec=true\nNoDisplay=true\nStartupWMClass={class}\n");
        if fs::read_to_string(&f).map(|t| t != text).unwrap_or(true) {
            let _ = fs::write(&f, text);
        }
    }
}

/// Пароль для конкретного сервера: 14 символов, латиница и цифры.
pub fn server_password(secret: &str, host: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, host.trim().to_lowercase().as_bytes());
    let hex: String = tag.as_ref().iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("Nx{hex}")
}

fn skin_store() -> PathBuf {
    data_dir().join("autoskin.json")
}

pub struct Auth {
    acct: String,
    secret: String,
    gamedir: PathBuf,
    skin: Option<(String, String)>, // (хеш скина, команда)
    host: Option<String>,
    joined: Option<Instant>,
    last_auth: Option<Instant>,
    last_cmd: HashMap<u8, Instant>,
    sent: u32,
    skin_done: bool,
    seq: u64,
    notify: Box<dyn Fn(&str, &str) + Send + Sync>,
}

impl Auth {
    pub fn new(acct: &str, gamedir: &Path, notify: Box<dyn Fn(&str, &str) + Send + Sync>) -> R<Auth> {
        let secret = accounts::ensure_secret(acct)?;
        Ok(Auth { acct: acct.into(), secret, gamedir: gamedir.into(), skin: accounts::skin_command(acct), host: None, joined: None, last_auth: None, last_cmd: HashMap::new(), sent: 0, skin_done: false, seq: 0, notify })
    }

    fn allowed(&self, skin: bool) -> bool {
        let s = load_settings();
        let on = if skin { s.auto_skin } else { s.auto_auth };
        on && self.host.as_ref().map(|h| !s.auto_skip.iter().any(|x| x == h)).unwrap_or(false)
    }

    fn send(&mut self, text: &str) {
        self.seq += 1;
        let dir = self.gamedir.join(".noxel");
        let _ = fs::create_dir_all(&dir);
        let line = format!("{}-{}\t{}\n", self.acct, self.seq, base64::engine::general_purpose::STANDARD.encode(text));
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("cmd.txt")) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// Вызывается для каждой строки лога игры.
    pub fn on_line(&mut self, line: &str) {
        if let Some(h) = crate::stats::server_from_log(line) {
            self.host = Some(h);
            self.joined = Some(Instant::now());
            self.last_auth = None;
            self.last_cmd.clear();
            self.sent = 0;
            self.skin_done = false;
            return;
        }
        let Some(i) = line.find("[CHAT]") else { return };
        let msg = line[i + 6..].trim();
        if msg.starts_with('<') || self.host.is_none() || !self.allowed(false) {
            return;
        }
        let low = msg.to_lowercase();
        let kind: u8 = if low.contains("/register") || low.contains("/reg ") {
            1
        } else if low.contains("/login") || low.contains("/l ") {
            2
        } else {
            return;
        };
        if self.sent >= 4 || self.last_cmd.get(&kind).map(|t| t.elapsed() < Duration::from_secs(4)).unwrap_or(false) {
            return;
        }
        let host = self.host.clone().unwrap();
        let pw = server_password(&self.secret, &host);
        let text = if kind == 1 {
            let after = &low[low.find("/reg").unwrap()..];
            if after.matches('<').count() == 1 && after.matches('>').count() == 1 { format!("/register {pw}") } else { format!("/register {pw} {pw}") }
        } else {
            format!("/login {pw}")
        };
        self.send(&text);
        self.last_cmd.insert(kind, Instant::now());
        self.last_auth = Some(Instant::now());
        self.sent += 1;
        (self.notify)(if kind == 1 { "register" } else { "login" }, &host);
    }

    /// Вызывается раз в полсекунды: отправляет команду скина после входа.
    pub fn tick(&mut self) {
        if self.skin_done || self.skin.is_none() || self.host.is_none() || !self.allowed(true) {
            return;
        }
        let ready = self.last_auth.map(|t| t + Duration::from_secs(5)).or(self.joined.map(|t| t + Duration::from_secs(10)));
        if !ready.map(|t| Instant::now() >= t).unwrap_or(false) {
            return;
        }
        self.skin_done = true;
        let (hash, cmd) = self.skin.clone().unwrap();
        let host = self.host.clone().unwrap();
        let mut store: HashMap<String, HashMap<String, String>> = read_json(&skin_store()).unwrap_or_default();
        if store.get(&self.acct).and_then(|m| m.get(&host)) == Some(&hash) {
            return; // этот скин на этом сервере уже ставился
        }
        store.entry(self.acct.clone()).or_default().insert(host.clone(), hash);
        write_json(&skin_store(), &store);
        self.send(&cmd);
        (self.notify)("skin", &host);
    }
}
