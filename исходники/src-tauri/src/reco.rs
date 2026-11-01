//! Рекомендуемые моды и подборки: список приходит с сервера Noxel, лаунчер сверяет его с каталогом и профилем.
use crate::mc;
use crate::mods;
use crate::util::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path, time::Duration};

const MR: &str = "https://api.modrinth.com/v2";

/// Для списка модов Modrinth: название, описание, значок, скачивания, есть ли файл под версию и загрузчик профиля и поставлен ли уже.
/// CurseForge-записи получают `ok: null` (наличие проверяется при установке).
pub fn info(profile_id: &str, items: &[(String, String)]) -> R<Value> {
    let p = mc::load_profiles().into_iter().find(|p| p.id == profile_id).ok_or("профиль не найден")?;
    let loaders = mods::loaders_of(&p.loader);
    let mr_ids: Vec<&String> = items.iter().filter(|(pr, _)| pr == "modrinth").map(|(_, id)| id).collect();
    let mut out = serde_json::Map::new();
    if !mr_ids.is_empty() {
        let ids = serde_json::to_string(&mr_ids).map_err(e)?;
        let r: Value = agent().get(&format!("{MR}/projects")).query("ids", &ids).call().map_err(e)?.into_json().map_err(e)?;
        let projects = r.as_array().cloned().unwrap_or_default();
        let lj = serde_json::to_string(&loaders).map_err(e)?;
        let gj = serde_json::to_string(&[&p.mc]).map_err(e)?;
        // наличие файла под версию и загрузчик профиля: по запросу на каждый проект, параллельно
        let checks: Vec<(String, bool, bool)> = std::thread::scope(|sc| {
            let hs: Vec<_> = projects
                .iter()
                .map(|pr| {
                    let (id, lj, gj, pid) = (pr["id"].as_str().unwrap_or("").to_string(), lj.clone(), gj.clone(), profile_id.to_string());
                    sc.spawn(move || {
                        let ok = agent()
                            .get(&format!("{MR}/project/{id}/version"))
                            .query("loaders", &lj)
                            .query("game_versions", &gj)
                            .timeout(Duration::from_secs(20))
                            .call()
                            .ok()
                            .and_then(|r| r.into_json::<Value>().ok())
                            .map(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
                            .unwrap_or(false);
                        let inst = mods::already(&pid, "mod", &id);
                        (id, ok, inst)
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (id, ok, inst) in checks {
            let pr = projects.iter().find(|x| x["id"] == id.as_str()).cloned().unwrap_or(Value::Null);
            let val = json!({"ok": ok, "title": pr["title"], "desc": pr["description"], "icon": pr["icon_url"], "downloads": pr["downloads"], "installed": inst});
            // и по id проекта, и по slug: на сервере может быть записано любое из них
            if let Some(sl) = pr["slug"].as_str() {
                out.insert(sl.to_string(), val.clone());
            }
            out.insert(id, val);
        }
    }
    for (pr, id) in items {
        if pr != "modrinth" {
            out.insert(id.clone(), json!({"ok": Value::Null, "installed": false}));
        }
    }
    Ok(Value::Object(out))
}

fn allowed_ext(name: &str) -> bool {
    let l = name.to_lowercase();
    [".json", ".emotecraft", ".emote", ".cpmmodel", ".cpmproject", ".png", ".zip", ".mcmeta", ".txt", ".md"].iter().any(|e| l.ends_with(e))
}

/// Скачивает файл (или архив с эмоциями и моделями) в папку профиля. Папка только из списка безопасных: моды сюда не попадают.
pub fn install_file(profile_id: &str, url: &str, folder: &str, extract: bool, prefix: &str, name: &str) -> R<Value> {
    if !["emotes", "player_models", "resourcepacks", "shaderpacks"].contains(&folder) {
        return Err("эта папка не разрешена".into());
    }
    if !url.starts_with("https://") {
        return Err("ссылка должна начинаться с https://".into());
    }
    let dir = mc::inst_dir(profile_id).join(folder);
    fs::create_dir_all(&dir).map_err(e)?;
    let tmp = data_dir().join("cache").join(format!("reco-{}.dl", now()));
    fs::create_dir_all(tmp.parent().unwrap()).map_err(e)?;
    download(url, &tmp, None)?;
    let res = if extract {
        let mut z = zip::ZipArchive::new(fs::File::open(&tmp).map_err(e)?).map_err(|_| "это не архив")?;
        let mut n = 0u32;
        for i in 0..z.len() {
            let mut ent = z.by_index(i).map_err(e)?;
            if ent.is_dir() {
                continue;
            }
            let Some(enc_name) = ent.enclosed_name() else { continue };
            let full = enc_name.to_string_lossy().replace('\\', "/");
            let rel = if prefix.is_empty() { full.clone() } else { match full.strip_prefix(prefix) { Some(r) => r.to_string(), None => continue } };
            if rel.is_empty() || !allowed_ext(&rel) || rel.contains("..") || rel.starts_with('/') {
                continue;
            }
            let out = dir.join(&rel);
            if let Some(par) = out.parent() {
                fs::create_dir_all(par).map_err(e)?;
            }
            let mut buf = vec![];
            ent.by_ref().take(20_000_000).read_to_end(&mut buf).map_err(e)?;
            fs::write(&out, buf).map_err(e)?;
            n += 1;
        }
        json!({"files": n})
    } else {
        let fname = Path::new(url.split('?').next().unwrap_or("")).file_name().map(|f| f.to_string_lossy().to_string()).filter(|f| allowed_ext(f)).unwrap_or_else(|| format!("{}.zip", name.replace(['/', '\\'], "_")));
        fs::copy(&tmp, dir.join(&fname)).map_err(e)?;
        json!({"files": 1, "name": fname})
    };
    let _ = fs::remove_file(&tmp);
    Ok(res)
}
