//! Косметика: скины для мечей и тотемов. Лаунчер сам собирает маленький ресурспак и включает его в профилях (моды не нужны).
use crate::util::*;
use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::Path;

const DATA: &str = include_str!("../assets/cosmetics.json");
const PACK: &str = "Noxel Cosmetics.zip";
const SWORDS: [&str; 7] = ["wooden_sword", "stone_sword", "copper_sword", "iron_sword", "golden_sword", "diamond_sword", "netherite_sword"];

fn all() -> Value {
    serde_json::from_str(DATA).unwrap_or(json!({"swords": [], "totems": []}))
}

fn cfg_path() -> std::path::PathBuf {
    data_dir().join("cosmetics.json")
}

pub fn catalog() -> Value {
    all()
}

pub fn get() -> Value {
    read_json::<Value>(&cfg_path()).filter(|v| v.is_object()).unwrap_or(json!({"sword": "", "totem": "", "swords": ["all"]}))
}

fn find(list: &Value, id: &str) -> Option<Value> {
    list.as_array()?.iter().find(|e| e["id"] == id).cloned()
}

fn put(z: &mut zip::ZipWriter<fs::File>, opt: zip::write::SimpleFileOptions, name: &str, item: &Value) -> R<()> {
    let png = base64::engine::general_purpose::STANDARD.decode(item["png"].as_str().unwrap_or("")).map_err(e)?;
    z.start_file(format!("assets/minecraft/textures/item/{name}.png"), opt).map_err(e)?;
    z.write_all(&png).map_err(e)?;
    if !item["mcmeta"].is_null() {
        z.start_file(format!("assets/minecraft/textures/item/{name}.png.mcmeta"), opt).map_err(e)?;
        z.write_all(item["mcmeta"].to_string().as_bytes()).map_err(e)?;
    }
    Ok(())
}

/// Собирает ресурспак по выбору и кладёт его в профиль, включая в options.txt.
fn build(dst: &Path, cfg: &Value) -> R<()> {
    let data = all();
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut z = zip::ZipWriter::new(fs::File::create(dst).map_err(e)?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("pack.mcmeta", opt).map_err(e)?;
    z.write_all(r#"{"pack":{"description":"Noxel: скины мечей и тотемов","min_format":88,"max_format":999}}"#.as_bytes()).map_err(e)?;
    let knife = &cfg["knife"];
    let kmodel = data["knives"]["models"].as_array().and_then(|a| a.iter().find(|x| x["id"] == knife["model"]).cloned());
    let ktex = kmodel.as_ref().and_then(|m| m["textures"].as_array().and_then(|a| a.iter().find(|t| t["id"] == knife["tex"]).cloned().or_else(|| a.first().cloned())));
    let only: Vec<String> = cfg["swords"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    if let (Some(km), Some(kt)) = (kmodel, ktex) {
        // готовая 3D-модель ножа (чужая работа с открытой лицензией, авторство в CREDITS.txt): модель, текстура и подмена моделей всех мечей
        z.start_file("assets/noxel/models/item/knife.json", opt).map_err(e)?;
        z.write_all(km["model"].as_str().unwrap_or("{}").as_bytes()).map_err(e)?;
        z.start_file("assets/noxel/textures/item/knife.png", opt).map_err(e)?;
        z.write_all(&b64.decode(kt["png"].as_str().unwrap_or("")).map_err(e)?).map_err(e)?;
        z.start_file("assets/noxel/CREDITS.txt", opt).map_err(e)?;
        z.write_all(format!("Модель: {} — автор {}, лицензия {}\nИсточник: {}\n", km["name"].as_str().unwrap_or(""), km["author"].as_str().unwrap_or(""), km["license"].as_str().unwrap_or(""), km["source"].as_str().unwrap_or("")).as_bytes()).map_err(e)?;
        for n in SWORDS {
            if only.is_empty() || only.iter().any(|x| x == "all" || x == n) {
                z.start_file(format!("assets/minecraft/items/{n}.json"), opt).map_err(e)?;
                z.write_all(br#"{"model":{"type":"minecraft:model","model":"noxel:item/knife"}}"#).map_err(e)?;
            }
        }
    } else if let Some(s) = find(&data["swords"], cfg["sword"].as_str().unwrap_or("")) {
        for n in SWORDS {
            if only.is_empty() || only.iter().any(|x| x == "all" || x == n) {
                put(&mut z, opt, n, &s)?;
            }
        }
    }
    if let Some(t) = find(&data["totems"], cfg["totem"].as_str().unwrap_or("")) {
        put(&mut z, opt, "totem_of_undying", &t)?;
    }
    z.finish().map_err(e)?;
    Ok(())
}

fn options_set(gamedir: &Path, on: bool) {
    options_pack(gamedir, PACK, on);
}

/// Включает или выключает ресурспак из папки resourcepacks в options.txt.
pub fn options_pack(gamedir: &Path, file: &str, on: bool) {
    let p = gamedir.join("options.txt");
    let name = format!("file/{file}");
    let txt = fs::read_to_string(&p).unwrap_or_default();
    let mut lines: Vec<String> = txt.lines().map(String::from).collect();
    let mut found = false;
    for l in lines.iter_mut() {
        if let Some(v) = l.strip_prefix("resourcePacks:") {
            found = true;
            let mut list: Vec<String> = serde_json::from_str(v).unwrap_or_default();
            list.retain(|x| x != &name);
            if on {
                if list.is_empty() {
                    list.push("vanilla".into());
                }
                list.push(name.clone());
            }
            *l = format!("resourcePacks:{}", serde_json::to_string(&list).unwrap_or("[]".into()));
        }
    }
    if !found && on {
        lines.push(format!("resourcePacks:[\"vanilla\",\"{name}\"]"));
    }
    let _ = fs::write(&p, lines.join("\n") + "\n");
}

/// Применяет выбор к одной папке игры. Пустой выбор снимает пак.
pub fn apply_to(gamedir: &Path) -> R<()> {
    let cfg = get();
    let any = !cfg["sword"].as_str().unwrap_or("").is_empty() || !cfg["knife"]["model"].as_str().unwrap_or("").is_empty() || !cfg["totem"].as_str().unwrap_or("").is_empty();
    let rp = gamedir.join("resourcepacks");
    let pack = rp.join(PACK);
    if !any {
        let _ = fs::remove_file(&pack);
        if gamedir.join("options.txt").exists() {
            options_set(gamedir, false);
        }
        return Ok(());
    }
    fs::create_dir_all(&rp).map_err(e)?;
    build(&pack, &cfg)?;
    options_set(gamedir, true);
    Ok(())
}

pub fn set(cfg: &Value) -> R<()> {
    write_json(&cfg_path(), cfg);
    for p in crate::mc::load_profiles() {
        let _ = apply_to(&crate::mc::inst_dir(&p.id));
    }
    Ok(())
}

/// Подборка 3D-ножей и оружия на Modrinth: лаунчер ничего не копирует, а скачивает пак у автора и включает его в профиле.
pub const PACKS: [&str; 18] = [
    "countercraft-knife-pack", "karambits", "karambits-x-punchy", "karambit-swords", "butterfly-knife", "swords-to-butterfly-knifes",
    "pvp-knife", "stiletto-knife", "daggers", "mini-daggers", "kamish-dagger", "custom-3d-swords", "alternate-3d-swords", "tactical-3d-tools",
    "dark-katana", "blood-scythe", "manepears-scythe", "cheetahs-weapons",
];

pub fn packs() -> R<Value> {
    let ids = serde_json::to_string(&PACKS).map_err(e)?;
    let r: Value = agent().get("https://api.modrinth.com/v2/projects").query("ids", &ids).call().map_err(e)?.into_json().map_err(e)?;
    let mut out = vec![];
    for p in r.as_array().cloned().unwrap_or_default() {
        out.push(json!({
            "id": p["id"], "slug": p["slug"], "title": p["title"], "desc": p["description"], "icon": p["icon_url"],
            "downloads": p["downloads"], "license": p["license"]["id"], "url": format!("https://modrinth.com/resourcepack/{}", p["slug"].as_str().unwrap_or("")),
        }));
    }
    out.sort_by_key(|x| std::cmp::Reverse(x["downloads"].as_u64().unwrap_or(0)));
    Ok(json!(out))
}

/// Ставит пак в профиль и сразу включает.
pub fn install_pack(project: &str, profile_id: &str, cb: &(dyn Fn(&str, f64, &str) + Sync)) -> R<String> {
    if !PACKS.contains(&project) && !project.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("неизвестный пак".into());
    }
    let dir = crate::mc::inst_dir(profile_id).join("resourcepacks");
    let before: std::collections::HashSet<String> = fs::read_dir(&dir).into_iter().flatten().flatten().map(|f| f.file_name().to_string_lossy().to_string()).collect();
    crate::mods::install("modrinth", "resourcepack", project, profile_id, None, cb)?;
    let mut fresh: Vec<(std::time::SystemTime, String)> = fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|f| {
        let n = f.file_name().to_string_lossy().to_string();
        if before.contains(&n) || n == PACK { return None; }
        Some((f.metadata().ok()?.modified().ok()?, n))
    }).collect();
    fresh.sort();
    let name = fresh.pop().map(|x| x.1).ok_or("пак уже стоял или не скачался: проверь папку resourcepacks")?;
    options_pack(&crate::mc::inst_dir(profile_id), &name, true);
    Ok(name)
}
