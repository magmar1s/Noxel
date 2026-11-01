//! Эмоции Emotecraft в папке профиля: список для лаунчера, чтение для предпросмотра, импорт файлов и архивов.
use crate::mc;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::{Path, PathBuf}};

/// Виды файлов: эмоции Emotecraft (папка emotes) и модели Customizable Player Models (папка player_models).
fn dir(profile_id: &str, kind: &str) -> PathBuf {
    mc::inst_dir(profile_id).join(if kind == "models" { "player_models" } else { "emotes" })
}

fn is_ok(kind: &str, name: &str) -> bool {
    let l = name.to_lowercase();
    if kind == "models" {
        l.ends_with(".cpmmodel") || l.ends_with(".cpmproject")
    } else {
        l.ends_with(".json") || l.ends_with(".emotecraft") || l.ends_with(".emote")
    }
}

fn truthy(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_str().map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false))
}

/// Все эмоции профиля. Для .json читаем название, автора и длину; файлы .emotecraft (двоичный формат) показываем без предпросмотра.
/// Эмоции с пометкой nsfw скрываются.
pub fn list(profile_id: &str, kind: &str) -> R<Value> {
    let d = dir(profile_id, kind);
    let mut out = vec![];
    let mut stack = vec![d.clone()];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = fs::read_dir(&cur) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let fname = e.file_name().to_string_lossy().to_string();
            if !is_ok(kind, &fname) {
                continue;
            }
            let rel = p.strip_prefix(&d).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if kind == "emotes" && fname.to_lowercase().ends_with(".json") {
                let Some(j) = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()) else { continue };
                let em = &j["emote"];
                if truthy(&em["nsfw"]) || truthy(&j["nsfw"]) {
                    continue;
                }
                out.push(json!({"file": rel, "name": j["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(fname.trim_end_matches(".json")), "author": j["author"].as_str().unwrap_or(""), "desc": j["description"].as_str().unwrap_or(""),
                                "loop": truthy(&em["isLoop"]), "ticks": em["endTick"].as_u64().unwrap_or(0), "preview": em["moves"].is_array(), "size": size}));
            } else {
                out.push(json!({"file": rel, "name": Path::new(&fname).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(fname.clone()), "author": "", "desc": "", "loop": false, "ticks": 0, "preview": false, "size": size}));
            }
        }
    }
    out.sort_by(|a, b| a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase()));
    Ok(json!(out))
}

fn safe_rel(kind: &str, rel: &str) -> R<PathBuf> {
    if rel.is_empty() || rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') || rel.contains('\0') || !is_ok(kind, rel) {
        return Err("недопустимое имя файла".into());
    }
    Ok(PathBuf::from(rel))
}

pub fn read(profile_id: &str, rel: &str) -> R<String> {
    let p = dir(profile_id, "emotes").join(safe_rel("emotes", rel)?);
    if !rel.to_lowercase().ends_with(".json") {
        return Err("предпросмотр есть только для эмоций в формате .json".into());
    }
    let mut s = String::new();
    fs::File::open(&p).map_err(|_| "файл не найден")?.take(3_000_000).read_to_string(&mut s).map_err(e)?;
    Ok(s)
}

pub fn delete(profile_id: &str, kind: &str, rel: &str) -> R<()> {
    fs::remove_file(dir(profile_id, kind).join(safe_rel(kind, rel)?)).map_err(|_| "не удалось удалить".to_string())
}

fn unique(d: &Path, name: &str) -> PathBuf {
    let mut p = d.join(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    let mut n = 2;
    while p.exists() {
        p = d.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    p
}

/// Добавляет эмоции из выбранных файлов: .json, .emotecraft, .emote или архив .zip с ними (внутри берутся только файлы эмоций).
pub fn import(profile_id: &str, kind: &str, paths: &[String]) -> R<Value> {
    let d = dir(profile_id, kind);
    fs::create_dir_all(&d).map_err(e)?;
    let (mut added, mut skipped) = (0u32, 0u32);
    for p in paths {
        let path = Path::new(p);
        let fname = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if is_ok(kind, &fname) {
            if fs::copy(path, unique(&d, &fname)).is_ok() { added += 1 } else { skipped += 1 }
        } else if fname.to_lowercase().ends_with(".zip") {
            let mut z = zip::ZipArchive::new(fs::File::open(path).map_err(e)?).map_err(|_| "не удалось открыть архив")?;
            for i in 0..z.len() {
                let mut ent = z.by_index(i).map_err(e)?;
                let Some(name) = ent.enclosed_name().and_then(|n| n.file_name().map(|f| f.to_string_lossy().to_string())) else { continue };
                if ent.is_dir() || !is_ok(kind, &name) {
                    continue;
                }
                let mut buf = vec![];
                if ent.by_ref().take(5_000_000).read_to_end(&mut buf).is_ok() && fs::write(unique(&d, &name), buf).is_ok() { added += 1 } else { skipped += 1 }
            }
        } else {
            skipped += 1;
        }
    }
    Ok(json!({"added": added, "skipped": skipped}))
}
