//! Визуалы Noxel: каталог модулей (его печатает сам мод при сборке) и общий файл настроек, который читает игра.
use crate::util::*;
use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const CATALOG: &str = include_str!("../assets/visuals-catalog.json");

pub fn path() -> PathBuf {
    data_dir().join("visuals.json")
}

pub fn catalog() -> Value {
    serde_json::from_str(CATALOG).unwrap_or(json!({"modules": [], "presets": {}, "themes": {}}))
}

pub fn load() -> Value {
    read_json::<Value>(&path()).filter(|v| v.is_object()).unwrap_or(json!({}))
}

pub fn save(v: &Value) -> R<()> {
    if !v.is_object() {
        return Err("настройки должны быть объектом".into());
    }
    if let Some(d) = path().parent() {
        let _ = fs::create_dir_all(d);
    }
    // лаунчер правит только свои разделы: метки и память сундуков, которые игра пишет в тот же файл, не трогаем
    let mut cur = load();
    if let (Some(n), Some(o)) = (v.as_object(), cur.as_object_mut()) {
        for k in ["modules", "style", "assistBlock"] {
            if let Some(x) = n.get(k) {
                o.insert(k.into(), x.clone());
            }
        }
    }
    fs::write(path(), serde_json::to_vec_pretty(&cur).map_err(e)?).map_err(e)
}

/// Код для обмена настройками: тот же «NXV1:…», что мод показывает в игре.
pub fn export() -> String {
    let mut v = load();
    if let Some(o) = v.as_object_mut() {
        o.remove("waypoints");
    }
    format!("NXV1:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&v).unwrap_or_default()))
}

pub fn import(code: &str) -> R<()> {
    let c = code.trim();
    let b = c.strip_prefix("NXV1:").ok_or("это не код настроек Noxel (должен начинаться с NXV1:)")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b.trim_end_matches('=')).map_err(|_| "код повреждён")?;
    let v: Value = serde_json::from_slice(&raw).map_err(|_| "код повреждён")?;
    let mut cur = load();
    if let (Some(n), Some(o)) = (v.as_object(), cur.as_object_mut()) {
        // метки и память о сундуках у каждого свои: из чужого кода берём только оформление и модули
        for k in ["modules", "style"] {
            if let Some(x) = n.get(k) {
                o.insert(k.into(), x.clone());
            }
        }
    } else {
        return Err("код повреждён".into());
    }
    save(&cur)
}
