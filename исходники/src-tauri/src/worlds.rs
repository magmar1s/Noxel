//! Миры профиля: список и резервные копии (zip) с восстановлением.
use crate::mc;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Write, path::{Path, PathBuf}};

fn backups_dir(profile: &str) -> PathBuf {
    data_dir().join("backups").join(mc::slug(profile))
}

fn safe_name(n: &str) -> R<&str> {
    if n.is_empty() || n.contains('/') || n.contains('\\') || n.contains("..") || n.contains('\0') {
        return Err("недопустимое имя".into());
    }
    Ok(n)
}

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => t += dir_size(&e.path()),
                Ok(m) => t += m.len(),
                _ => {}
            }
        }
    }
    t
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// Имя «мир-20261031-120501.zip» -> (мир, время)
fn parse_backup(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".zip")?;
    let cut = stem.rfind('-')?;
    let cut2 = stem[..cut].rfind('-')?;
    Some(stem[..cut2].to_string())
}

pub fn list(profile: &str) -> R<Value> {
    let saves = mc::inst_dir(profile).join("saves");
    let bk = backups_dir(profile);
    let names: Vec<String> = fs::read_dir(&bk).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    let mut worlds = vec![];
    if let Ok(rd) = fs::read_dir(&saves) {
        for e in rd.flatten() {
            if !e.path().is_dir() || !e.path().join("level.dat").exists() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let n = names.iter().filter(|f| parse_backup(f).as_deref() == Some(name.as_str())).count();
            worlds.push(json!({"name": name, "size": dir_size(&e.path()), "modified": mtime(&e.path().join("level.dat")), "backups": n}));
        }
    }
    worlds.sort_by_key(|w| std::cmp::Reverse(w["modified"].as_u64().unwrap_or(0)));
    let mut backups: Vec<Value> = names.iter().filter_map(|f| {
        let w = parse_backup(f)?;
        let p = bk.join(f);
        Some(json!({"file": f, "world": w, "size": fs::metadata(&p).map(|m| m.len()).unwrap_or(0), "created": mtime(&p)}))
    }).collect();
    backups.sort_by_key(|b| std::cmp::Reverse(b["created"].as_u64().unwrap_or(0)));
    Ok(json!({"worlds": worlds, "backups": backups}))
}

fn add_dir(zw: &mut zip::ZipWriter<fs::File>, root: &Path, cur: &Path, prefix: &str) -> R<()> {
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for ent in fs::read_dir(cur).map_err(e)?.flatten() {
        let p = ent.path();
        let rel = p.strip_prefix(root).map_err(e)?.to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            add_dir(zw, root, &p, prefix)?;
        } else if p.file_name().map(|n| n != "session.lock").unwrap_or(true) {
            zw.start_file(format!("{prefix}/{rel}"), opt).map_err(e)?;
            let mut f = fs::File::open(&p).map_err(e)?;
            std::io::copy(&mut f, zw).map_err(e)?;
        }
    }
    Ok(())
}

pub fn backup(profile: &str, world: &str) -> R<Value> {
    let world = safe_name(world)?;
    let src = mc::inst_dir(profile).join("saves").join(world);
    if !src.join("level.dat").exists() {
        return Err("мир не найден".into());
    }
    let t = now();
    let stamp = {
        // YYYYmmdd-HHMMSS по UTC без внешних библиотек
        let (days, secs) = ((t / 86400) as i64, t % 86400);
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z.rem_euclid(146097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
    };
    let dir = backups_dir(profile);
    fs::create_dir_all(&dir).map_err(e)?;
    let file = format!("{world}-{stamp}.zip");
    let out = dir.join(&file);
    let mut zw = zip::ZipWriter::new(fs::File::create(&out).map_err(e)?);
    add_dir(&mut zw, &src, &src, world)?;
    zw.finish().map_err(e)?.flush().map_err(e)?;
    Ok(json!({"file": file, "size": fs::metadata(&out).map(|m| m.len()).unwrap_or(0)}))
}

pub fn restore(profile: &str, file: &str) -> R<Value> {
    let file = safe_name(file)?;
    let world = parse_backup(file).ok_or("неверное имя копии")?;
    let zp = backups_dir(profile).join(file);
    let saves = mc::inst_dir(profile).join("saves");
    let target = saves.join(&world);
    if target.exists() {
        // текущий мир не теряем: переименовываем рядом
        let mut n = 1;
        let mut aside = saves.join(format!("{world}-до-восстановления"));
        while aside.exists() {
            n += 1;
            aside = saves.join(format!("{world}-до-восстановления-{n}"));
        }
        fs::rename(&target, &aside).map_err(e)?;
    }
    let mut z = zip::ZipArchive::new(fs::File::open(&zp).map_err(|_| "копия не найдена")?).map_err(e)?;
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(rel) = ent.enclosed_name() else { continue };
        let out = saves.join(&rel);
        if !out.starts_with(&saves) {
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        std::io::copy(&mut ent, &mut fs::File::create(&out).map_err(e)?).map_err(e)?;
    }
    Ok(json!({"world": world}))
}

pub fn delete(profile: &str, file: &str) -> R<()> {
    let file = safe_name(file)?;
    fs::remove_file(backups_dir(profile).join(file)).map_err(|_| "не удалось удалить копию".to_string())
}
