//! Общее хранилище файлов по хешу и точки восстановления профилей.
//!
//! Одинаковые моды, ресурспаки и шейдеры в разных профилях хранятся на диске один раз:
//! файл профиля становится жёсткой ссылкой на копию в `store/`. Лаунчер всегда пишет
//! файлы через временный файл и переименование, поэтому ссылки никогда не меняются «на месте».
use crate::mc::{inst_dir, load_profiles};
use crate::util::*;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const KINDS: &[&str] = &["mods", "resourcepacks", "shaderpacks"];

fn store_dir() -> PathBuf {
    data_dir().join("store")
}

pub fn sha1_file(p: &Path) -> R<String> {
    let mut f = fs::File::open(p).map_err(e)?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(e)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}
#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn links(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.nlink()
}
#[cfg(not(unix))]
fn links(_: &fs::Metadata) -> u64 {
    2
}

fn is_content(name: &str) -> bool {
    let n = name.strip_suffix(".disabled").unwrap_or(name).to_lowercase();
    n.ends_with(".jar") || n.ends_with(".zip")
}

/// Сводит один файл к общей копии. Возвращает сэкономленные байты.
pub fn dedupe_file(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    if !md.is_file() || md.len() < 4096 {
        return 0;
    }
    let Ok(h) = sha1_file(p) else { return 0 };
    let sp = store_dir().join(&h[..2]).join(&h);
    match fs::metadata(&sp) {
        Ok(sm) if sm.len() == md.len() => {
            if same_file(&sm, &md) {
                return 0;
            }
            let tmp = p.with_extension("noxel-link");
            if fs::hard_link(&sp, &tmp).is_ok() {
                if fs::rename(&tmp, p).is_ok() {
                    return md.len();
                }
                let _ = fs::remove_file(&tmp);
            }
            0
        }
        _ => {
            let _ = fs::create_dir_all(sp.parent().unwrap());
            let _ = fs::remove_file(&sp);
            let _ = fs::hard_link(p, &sp);
            0
        }
    }
}

/// Сводит к общим копиям файлы одного профиля. Возвращает сэкономленные байты.
pub fn dedupe_profile(profile_id: &str) -> u64 {
    let dir = inst_dir(profile_id);
    let mut saved = 0;
    for k in KINDS {
        let Ok(rd) = fs::read_dir(dir.join(k)) else { continue };
        for f in rd.flatten() {
            if is_content(&f.file_name().to_string_lossy()) {
                saved += dedupe_file(&f.path());
            }
        }
    }
    saved
}

/// Удаляет из хранилища файлы, на которые больше не ссылается ни один профиль.
pub fn gc() -> u64 {
    let mut freed = 0;
    let Ok(top) = fs::read_dir(store_dir()) else { return 0 };
    for d in top.flatten() {
        let Ok(rd) = fs::read_dir(d.path()) else { continue };
        for f in rd.flatten() {
            if let Ok(m) = f.metadata() {
                if m.is_file() && links(&m) <= 1 {
                    freed += m.len();
                    let _ = fs::remove_file(f.path());
                }
            }
        }
    }
    freed
}

/// Кнопка «Освободить место»: сводит одинаковые файлы всех профилей и чистит хранилище.
pub fn dedupe_all() -> Value {
    let mut saved = 0;
    for p in load_profiles() {
        saved += dedupe_profile(&p.id);
    }
    let freed = gc();
    json!({"saved": saved, "freed": freed, "store": store_size()})
}

pub fn store_size() -> u64 {
    let mut n = 0;
    if let Ok(top) = fs::read_dir(store_dir()) {
        for d in top.flatten() {
            if let Ok(rd) = fs::read_dir(d.path()) {
                n += rd.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum::<u64>();
            }
        }
    }
    n
}

/// Ищет на Modrinth моды, у которых нет записи «откуда пришёл» (импорт, свои файлы),
/// и записывает их как модриновские: так у них появятся обновления, иконки и передача другу.
pub fn index_unknown(profile_id: &str) -> usize {
    let dir = inst_dir(profile_id);
    let known: std::collections::HashSet<String> = crate::mods::indexed_files(profile_id).into_iter().collect();
    let mut by_hash: Vec<(String, String, String)> = vec![]; // (hash, file, kind)
    for (k, kind) in [("mods", "mod"), ("resourcepacks", "resourcepack"), ("shaderpacks", "shader")] {
        let Ok(rd) = fs::read_dir(dir.join(k)) else { continue };
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let base = name.strip_suffix(".disabled").unwrap_or(&name).to_string();
            if !is_content(&name) || known.contains(&base) || !f.path().is_file() {
                continue;
            }
            if let Ok(h) = sha1_file(&f.path()) {
                by_hash.push((h, base, kind.to_string()));
            }
        }
    }
    if by_hash.is_empty() {
        return 0;
    }
    let mut found = 0;
    for chunk in by_hash.chunks(400) {
        let hashes: Vec<&str> = chunk.iter().map(|x| x.0.as_str()).collect();
        let Ok(r) = agent().post("https://api.modrinth.com/v2/version_files").send_json(json!({"hashes": hashes, "algorithm": "sha1"})) else { continue };
        let Ok(vers) = r.into_json::<Value>() else { continue };
        let pids: Vec<String> = vers.as_object().map(|o| o.values().filter_map(|v| v["project_id"].as_str().map(String::from)).collect()).unwrap_or_default();
        let projects: Value = if pids.is_empty() {
            json!([])
        } else {
            agent().get("https://api.modrinth.com/v2/projects").query("ids", &serde_json::to_string(&pids).unwrap_or_default()).call().ok().and_then(|r| r.into_json().ok()).unwrap_or(json!([]))
        };
        let mut items = vec![];
        for (h, file, kind) in chunk {
            let v = &vers[h.as_str()];
            let Some(pid) = v["project_id"].as_str() else { continue };
            let pr = projects.as_array().and_then(|a| a.iter().find(|p| p["id"] == pid)).cloned().unwrap_or(json!({}));
            items.push((file.clone(), pid.to_string(), pr["title"].as_str().unwrap_or(file).to_string(), pr["icon_url"].as_str().map(String::from), v["version_number"].as_str().unwrap_or("").to_string(), kind.clone()));
        }
        found += items.len();
        crate::mods::remember_many(profile_id, items);
    }
    found
}

// ── точки восстановления ──

fn snaps_dir(profile_id: &str) -> PathBuf {
    inst_dir(profile_id).join(".noxel/snapshots")
}

fn copy_tree(from: &Path, to: &Path, link: bool) -> u64 {
    let mut n = 0;
    let Ok(rd) = fs::read_dir(from) else { return 0 };
    let _ = fs::create_dir_all(to);
    for f in rd.flatten() {
        let (src, dst) = (f.path(), to.join(f.file_name()));
        let Ok(md) = fs::symlink_metadata(&src) else { continue };
        if md.is_dir() {
            n += copy_tree(&src, &dst, link);
        } else if md.is_file() {
            if !(link && fs::hard_link(&src, &dst).is_ok()) && fs::copy(&src, &dst).is_err() {
                continue;
            }
            n += 1;
        }
    }
    n
}

/// Сохраняет моды, ресурспаки, шейдеры, настройки модов и список источников.
/// Файлы модов не копируются, а связываются жёсткими ссылками, поэтому точка почти не занимает места.
pub fn snapshot(profile_id: &str, reason: &str) -> R<Value> {
    safe_name(profile_id)?;
    let dir = inst_dir(profile_id);
    let id = format!("{}", now_ms());
    let sd = snaps_dir(profile_id).join(&id);
    fs::create_dir_all(&sd).map_err(e)?;
    let mut files = 0;
    for k in KINDS {
        files += copy_tree(&dir.join(k), &sd.join(k), true);
    }
    files += copy_tree(&dir.join("config"), &sd.join("config"), false);
    for f in ["options.txt", ".noxel/index.json", ".noxel/pack.json"] {
        if dir.join(f).is_file() {
            let _ = fs::create_dir_all(sd.join(f).parent().unwrap());
            let _ = fs::copy(dir.join(f), sd.join(f));
        }
    }
    let meta = json!({"id": id, "reason": reason, "created": now(), "files": files, "mods": count(&dir.join("mods"))});
    write_json(&sd.join("snapshot.json"), &meta);
    // храним последние 8 точек
    let mut all = list_raw(profile_id);
    while all.len() > 8 {
        if let Some(old) = all.pop() {
            let _ = fs::remove_dir_all(snaps_dir(profile_id).join(old["id"].as_str().unwrap_or("x")));
        }
    }
    Ok(meta)
}

fn count(d: &Path) -> usize {
    fs::read_dir(d).map(|r| r.flatten().filter(|f| is_content(&f.file_name().to_string_lossy())).count()).unwrap_or(0)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn list_raw(profile_id: &str) -> Vec<Value> {
    let mut v: Vec<Value> = fs::read_dir(snaps_dir(profile_id))
        .map(|r| r.flatten().filter_map(|f| read_json::<Value>(&f.path().join("snapshot.json"))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b["id"].as_str().unwrap_or("").cmp(a["id"].as_str().unwrap_or("")));
    v
}

pub fn snapshots(profile_id: &str) -> R<Value> {
    safe_name(profile_id)?;
    Ok(json!(list_raw(profile_id)))
}

/// Возвращает профиль к точке. Перед этим сохраняет текущее состояние, чтобы откат тоже можно было отменить.
pub fn restore(profile_id: &str, snap: &str) -> R<Value> {
    safe_name(profile_id)?;
    safe_name(snap)?;
    let sd = snaps_dir(profile_id).join(snap);
    if !sd.join("snapshot.json").is_file() {
        return Err("точка восстановления не найдена".into());
    }
    snapshot(profile_id, "перед откатом")?;
    let dir = inst_dir(profile_id);
    for k in KINDS.iter().chain(["config"].iter()) {
        let _ = fs::remove_dir_all(dir.join(k));
        copy_tree(&sd.join(k), &dir.join(k), *k != "config");
    }
    for f in ["options.txt", ".noxel/index.json", ".noxel/pack.json"] {
        if sd.join(f).is_file() {
            let _ = fs::copy(sd.join(f), dir.join(f));
        } else if f == ".noxel/pack.json" {
            let _ = fs::remove_file(dir.join(f));
        }
    }
    Ok(json!({"ok": true, "mods": count(&dir.join("mods"))}))
}

pub fn delete_snapshot(profile_id: &str, snap: &str) -> R<()> {
    safe_name(profile_id)?;
    safe_name(snap)?;
    fs::remove_dir_all(snaps_dir(profile_id).join(snap)).map_err(e)
}
