//! Локальный API лаунчера для мода Noxel внутри игры: слушает только 127.0.0.1, каждый запрос подписан ключом запуска.
//! Токен аккаунта Noxel мод не видит: все действия (друзья, мир, облако) выполняет лаунчер.
use crate::util::*;
use crate::{accounts, mc, social, tunnel};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::OnceLock,
    thread,
    time::Duration,
};

static INFO: OnceLock<(u16, String)> = OnceLock::new();

/// (порт, ключ), если API запущен.
pub fn info() -> Option<(u16, String)> {
    INFO.get().cloned()
}

pub fn start() {
    INFO.get_or_init(|| {
        let l = TcpListener::bind("127.0.0.1:0").expect("локальный порт для мода");
        let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
        let mut raw = [0u8; 16];
        let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut raw);
        let key: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let k2 = key.clone();
        thread::spawn(move || {
            for c in l.incoming().flatten() {
                let k = k2.clone();
                thread::spawn(move || handle(c, &k));
            }
        });
        (port, key)
    });
}

fn reply(s: &mut TcpStream, code: u16, v: &Value) {
    let body = v.to_string();
    let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

fn handle(mut s: TcpStream, key: &str) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = vec![];
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        match s.read(&mut tmp) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 16_384 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let hdr = |n: &str| lines.clone().find_map(|l| l.to_lowercase().strip_prefix(&format!("{n}:")).map(|v| v.trim().to_string()));
    if hdr("x-noxel-key").as_deref() != Some(key) {
        return reply(&mut s, 403, &json!({"error": "нет доступа"}));
    }
    let len: usize = hdr("content-length").and_then(|v| v.parse().ok()).unwrap_or(0).min(65_536);
    while buf.len() < head_end + len {
        match s.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let body: Value = serde_json::from_slice(&buf[head_end.min(buf.len())..]).unwrap_or(Value::Null);
    match route(&method, &path, &body) {
        Ok(v) => reply(&mut s, 200, &v),
        Err(x) => reply(&mut s, 400, &json!({"error": x})),
    }
}

fn account() -> R<(String, String)> {
    let a = accounts::active_fresh()?;
    let t = accounts::noxel_token(&a.id)?;
    Ok((a.id, t))
}

fn route(method: &str, path: &str, body: &Value) -> R<Value> {
    let profile = mc::RUNNING.lock().unwrap().clone();
    match (method, path) {
        ("GET", "/ping") => Ok(json!({"ok": true, "profile": profile})),
        ("GET", "/friends") => {
            let (_, t) = account()?;
            let mut v = social::friends(&t)?;
            v["room"] = tunnel::state();
            Ok(v)
        }
        ("POST", "/friend") => {
            let (_, t) = account()?;
            social::friend_action(&t, body["action"].as_str().unwrap_or("add"), body["name"].as_str().unwrap_or(""))
        }
        ("POST", "/room/open") => {
            let (_, t) = account()?;
            tunnel::open_world(&t, &profile.ok_or("игра не запущена лаунчером")?)
        }
        ("POST", "/room/close") => {
            tunnel::close_world(profile.as_deref());
            Ok(json!({"ok": true}))
        }
        ("POST", "/room/join") => {
            let (_, t) = account()?;
            let port = tunnel::guest_open(&t, body["host"].as_str().unwrap_or(""))?;
            Ok(json!({"port": port}))
        }
        ("POST", "/cloud/save") => {
            // мир, в который играют сейчас, это самый свежий по времени сохранения
            let (_, t) = account()?;
            let pid = profile.ok_or("игра не запущена лаунчером")?;
            let w = crate::worlds::list(&pid)?;
            let name = w["worlds"][0]["name"].as_str().ok_or("в профиле нет миров")?.to_string();
            let mcv = mc::load_profiles().into_iter().find(|p| p.id == pid).map(|p| p.mc).unwrap_or_default();
            social::cloud_upload(&t, &pid, &mcv, &name)?;
            let list = social::cloud_list(&t)?;
            let id = list["worlds"][0]["id"].as_u64().ok_or("мир не появился в облаке")?;
            let url = social::cloud_share(&t, id)?["url"].clone();
            Ok(json!({"name": name, "url": url}))
        }
        _ => Err("неизвестный запрос".into()),
    }
}
