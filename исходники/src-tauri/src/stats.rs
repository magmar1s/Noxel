//! Время игры, заходы на серверы и миры. Хранится в данные/stats.json и не пропадает при перезапуске.
use crate::util::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, sync::Mutex};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Session {
    pub id: String,
    pub start: u64,
    pub secs: u64,
    pub profile: String,
    pub mc: String,
    pub loader: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

type Store = HashMap<String, Vec<Session>>;

fn path() -> std::path::PathBuf {
    data_dir().join("stats.json")
}

fn load() -> Store {
    read_json(&path()).unwrap_or_default()
}

pub fn sessions(acc: &str) -> Vec<Session> {
    load().remove(acc).unwrap_or_default()
}

pub fn begin(acc: &str, profile: &str, mc: &str, loader: &str) -> String {
    let _g = LOCK.lock().unwrap();
    let mut s = load();
    let id = format!("{}-{}", now(), profile);
    s.entry(acc.to_string()).or_default().push(Session { id: id.clone(), start: now(), secs: 0, profile: profile.into(), mc: mc.into(), loader: loader.into(), servers: vec![] });
    write_json(&path(), &s);
    id
}

pub fn update(acc: &str, id: &str, f: impl FnOnce(&mut Session)) {
    let _g = LOCK.lock().unwrap();
    let mut s = load();
    if let Some(x) = s.get_mut(acc).and_then(|v| v.iter_mut().find(|x| x.id == id)) {
        f(x);
        write_json(&path(), &s);
    }
}

/// Объединяет сессии из облака с локальными (по id, поэтому повторная синхронизация ничего не удваивает).
pub fn merge(acc: &str, remote: Vec<Session>) {
    let _g = LOCK.lock().unwrap();
    let mut s = load();
    let list = s.entry(acc.to_string()).or_default();
    for r in remote {
        match list.iter_mut().find(|x| x.id == r.id) {
            Some(x) if r.secs > x.secs => *x = r,
            Some(_) => {}
            None => list.push(r),
        }
    }
    list.sort_by_key(|x| x.start);
    write_json(&path(), &s);
}

/// Хост из строки лога вида «Connecting to play.example.com, 25565».
pub fn server_from_log(line: &str) -> Option<String> {
    let rest = &line[line.find("Connecting to ")? + 14..];
    let host = rest.split(',').next()?.trim();
    if host.is_empty() || host.len() > 100 || host.contains(' ') { None } else { Some(host.to_lowercase()) }
}

pub fn summary(acc: &str) -> Value {
    let ss = sessions(acc);
    let total: u64 = ss.iter().map(|s| s.secs).sum();
    let mut by_ver: HashMap<String, u64> = HashMap::new();
    let mut srv: HashMap<String, (u32, u64)> = HashMap::new();
    for s in &ss {
        *by_ver.entry(s.mc.clone()).or_default() += s.secs;
        for h in &s.servers {
            let e = srv.entry(h.clone()).or_insert((0, 0));
            e.0 += 1;
            e.1 = e.1.max(s.start);
        }
    }
    let mut vers: Vec<_> = by_ver.into_iter().collect();
    vers.sort_by(|a, b| b.1.cmp(&a.1));
    let mut servers: Vec<_> = srv.into_iter().collect();
    servers.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
    json!({
        "total_secs": total, "sessions": ss.len(), "raw": ss.iter().rev().take(400).map(|s| json!([s.start, s.secs])).collect::<Vec<_>>(), "last": ss.iter().map(|s| s.start).max(),
        "versions": vers.iter().take(8).map(|(v, s)| json!({"mc": v, "secs": s})).collect::<Vec<_>>(),
        "servers": servers.iter().take(12).map(|(h, (n, l))| json!({"host": h, "joins": n, "last": l})).collect::<Vec<_>>(),
    })
}

/// Локальные миры всех профилей: название папки и время последнего изменения.
pub fn worlds() -> Vec<Value> {
    let mut out: Vec<(u64, Value)> = vec![];
    for p in crate::mc::load_profiles() {
        let saves = crate::mc::inst_dir(&p.id).join("saves");
        let Ok(rd) = fs::read_dir(&saves) else { continue };
        for w in rd.flatten() {
            if !w.path().join("level.dat").exists() {
                continue;
            }
            let m = w.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push((m, json!({"name": w.file_name().to_string_lossy(), "profile": p.name, "mc": p.mc, "modified": m})));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().take(12).map(|x| x.1).collect()
}
