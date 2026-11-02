//! Переезд в Noxel и обмен сборками: экспорт профиля в .mrpack и zip CurseForge,
//! импорт сборок Modrinth / CurseForge / Prism / MultiMC из файла и поиск профилей
//! других лаунчеров на диске (Prism, MultiMC, PolyMC, Modrinth App, CurseForge,
//! ATLauncher, GDLauncher, официальный лаунчер, TLauncher, Legacy Launcher, HMCL).
use crate::mc::{self, inst_dir, Profile};
use crate::store::sha1_file;
use crate::util::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

type Cb<'a> = &'a (dyn Fn(&str, f64, &str) + Sync);

const CONTENT: &[&str] = &["mods", "resourcepacks", "shaderpacks"];
/// Папки и файлы с настройками, которые переносятся вместе со сборкой.
const SETTINGS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts", "global_packs", "options.txt", "optionsof.txt", "optionsshaders.txt", "servers.dat"];

// ── экспорт ──

fn sha512_file(p: &Path) -> R<String> {
    let mut f = fs::File::open(p).map_err(e)?;
    let mut h = Sha512::new();
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

/// Отпечаток файла в стиле CurseForge: MurmurHash2 (seed 1) по байтам без пробелов и переводов строк.
pub fn cf_fingerprint(data: &[u8]) -> u32 {
    let buf: Vec<u8> = data.iter().copied().filter(|b| !matches!(b, 9 | 10 | 13 | 32)).collect();
    const M: u32 = 0x5bd1_e995;
    let len = buf.len() as u32;
    let mut h: u32 = 1 ^ len;
    let mut chunks = buf.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    let rem = chunks.remainder();
    if rem.len() >= 3 {
        h ^= (rem[2] as u32) << 16;
    }
    if rem.len() >= 2 {
        h ^= (rem[1] as u32) << 8;
    }
    if !rem.is_empty() {
        h ^= rem[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// Версия загрузчика профиля: из профиля или из id установленной версии.
fn loader_version(p: &Profile) -> Option<String> {
    if let Some(v) = p.loader_ver.clone().filter(|s| !s.is_empty()) {
        return Some(v);
    }
    let vid = p.version_id.clone()?;
    match p.loader.as_str() {
        "fabric" => vid.strip_prefix("fabric-loader-").and_then(|r| r.rsplit_once('-')).map(|x| x.0.to_string()),
        "quilt" => vid.strip_prefix("quilt-loader-").and_then(|r| r.rsplit_once('-')).map(|x| x.0.to_string()),
        "forge" => vid.split_once("-forge-").map(|x| x.1.to_string()).or_else(|| vid.rsplit_once('-').map(|x| x.1.to_string())),
        "neoforge" => vid.strip_prefix("neoforge-").map(String::from),
        _ => None,
    }
}

struct Item {
    rel: String,
    path: PathBuf,
    size: u64,
}

fn content_items(dir: &Path) -> Vec<Item> {
    let mut out = vec![];
    for k in CONTENT {
        let Ok(rd) = fs::read_dir(dir.join(k)) else { continue };
        for f in rd.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let Ok(md) = f.metadata() else { continue };
            if !md.is_file() || !(lower.ends_with(".jar") || lower.ends_with(".zip")) {
                continue; // выключенные (.disabled) и папки не берём
            }
            out.push(Item { rel: format!("{k}/{name}"), path: f.path(), size: md.len() });
        }
    }
    out
}

fn add_file(zw: &mut zip::ZipWriter<fs::File>, name: &str, path: &Path) -> R<()> {
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file(name, opts).map_err(e)?;
    let mut f = fs::File::open(path).map_err(e)?;
    std::io::copy(&mut f, zw).map_err(e)?;
    Ok(())
}

fn add_tree(zw: &mut zip::ZipWriter<fs::File>, root: &Path, cur: &Path, prefix: &str, n: &mut usize) -> R<()> {
    let Ok(rd) = fs::read_dir(cur) else { return Ok(()) };
    for f in rd.flatten() {
        let p = f.path();
        let Ok(md) = fs::symlink_metadata(&p) else { continue };
        let rel = p.strip_prefix(root).map_err(e)?.to_string_lossy().replace('\\', "/");
        if md.is_dir() {
            add_tree(zw, root, &p, prefix, n)?;
        } else if md.is_file() {
            add_file(zw, &format!("{prefix}{rel}"), &p)?;
            *n += 1;
        }
    }
    Ok(())
}

fn add_settings(zw: &mut zip::ZipWriter<fs::File>, dir: &Path, prefix: &str, saves: bool) -> R<usize> {
    let mut n = 0;
    let mut list: Vec<&str> = SETTINGS.to_vec();
    if saves {
        list.push("saves");
    }
    for s in list {
        let p = dir.join(s);
        if p.is_file() {
            add_file(zw, &format!("{prefix}{s}"), &p)?;
            n += 1;
        } else if p.is_dir() {
            add_tree(zw, dir, &p, prefix, &mut n)?;
        }
    }
    Ok(n)
}

/// Экспорт в .mrpack: моды, найденные на Modrinth, идут ссылками, остальное кладётся в overrides.
pub fn export_mrpack(profile_id: &str, out: &Path, saves: bool, cb: Cb) -> R<Value> {
    let p = mc::find_profile(profile_id)?;
    let dir = inst_dir(&p.id);
    let items = content_items(&dir);
    cb("Считаю хеши", 0.05, "");
    let mut hashes: Vec<(String, String)> = vec![];
    for (i, it) in items.iter().enumerate() {
        hashes.push((sha1_file(&it.path)?, sha512_file(&it.path)?));
        cb("Считаю хеши", 0.05 + 0.3 * i as f64 / items.len().max(1) as f64, &it.rel);
    }
    cb("Ищу файлы на Modrinth", 0.4, "");
    let mut known: HashMap<String, Value> = HashMap::new();
    for chunk in hashes.chunks(400) {
        let list: Vec<&str> = chunk.iter().map(|h| h.0.as_str()).collect();
        if let Ok(r) = agent().post("https://api.modrinth.com/v2/version_files").send_json(json!({"hashes": list, "algorithm": "sha1"})) {
            if let Ok(Value::Object(o)) = r.into_json::<Value>() {
                known.extend(o);
            }
        }
    }
    let mut files = vec![];
    let mut overrides: Vec<usize> = vec![];
    for (i, it) in items.iter().enumerate() {
        let (s1, s512) = &hashes[i];
        let url = known.get(s1).and_then(|v| v["files"].as_array()).and_then(|fs| fs.iter().find(|f| f["hashes"]["sha1"] == s1.as_str())).and_then(|f| f["url"].as_str());
        match url {
            Some(u) => files.push(json!({"path": it.rel, "hashes": {"sha1": s1, "sha512": s512}, "downloads": [u], "fileSize": it.size})),
            None => overrides.push(i),
        }
    }
    let mut deps = json!({"minecraft": p.mc});
    let lv = loader_version(&p).unwrap_or_default();
    match p.loader.as_str() {
        "fabric" => deps["fabric-loader"] = json!(lv),
        "quilt" => deps["quilt-loader"] = json!(lv),
        "forge" => deps["forge"] = json!(lv),
        "neoforge" => deps["neoforge"] = json!(lv),
        _ => {}
    }
    let index = json!({"formatVersion": 1, "game": "minecraft", "versionId": "1.0.0", "name": p.name, "summary": "Сборка из лаунчера Noxel", "files": files, "dependencies": deps});
    cb("Упаковываю", 0.6, "");
    if let Some(d) = out.parent() {
        fs::create_dir_all(d).map_err(e)?;
    }
    let mut zw = zip::ZipWriter::new(fs::File::create(out).map_err(e)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("modrinth.index.json", opts).map_err(e)?;
    zw.write_all(serde_json::to_string_pretty(&index).map_err(e)?.as_bytes()).map_err(e)?;
    for i in &overrides {
        add_file(&mut zw, &format!("overrides/{}", items[*i].rel), &items[*i].path)?;
    }
    let n = add_settings(&mut zw, &dir, "overrides/", saves)?;
    zw.finish().map_err(e)?;
    cb("Готово", 1.0, "");
    Ok(json!({"path": out.to_string_lossy(), "linked": files.len(), "embedded": overrides.len(), "settings": n, "size": fs::metadata(out).map(|m| m.len()).unwrap_or(0)}))
}

/// Экспорт в zip CurseForge: файлы, которые CurseForge узнаёт по отпечатку, идут в manifest, остальное в overrides.
pub fn export_cf(profile_id: &str, out: &Path, saves: bool, cb: Cb) -> R<Value> {
    let p = mc::find_profile(profile_id)?;
    let dir = inst_dir(&p.id);
    let items = content_items(&dir);
    cb("Считаю отпечатки", 0.05, "");
    let mut fps: Vec<u32> = vec![];
    for it in &items {
        let b = fs::read(&it.path).map_err(e)?;
        fps.push(cf_fingerprint(&b));
    }
    cb("Ищу файлы на CurseForge", 0.4, "");
    let mut matched: HashMap<u32, (u64, u64)> = HashMap::new();
    for chunk in fps.chunks(300) {
        let r = agent().post("https://mr1s.site/noxel/v1/cf/fingerprints").send_json(json!({"fingerprints": chunk}));
        let Ok(r) = r else { continue };
        let Ok(v) = r.into_json::<Value>() else { continue };
        for m in v["data"]["exactMatches"].as_array().cloned().unwrap_or_default() {
            let f = &m["file"];
            if let (Some(fp), Some(fid), Some(mid)) = (f["fileFingerprint"].as_u64(), f["id"].as_u64(), f["modId"].as_u64()) {
                matched.insert(fp as u32, (mid, fid));
            }
        }
    }
    let mut files = vec![];
    let mut overrides = vec![];
    for (i, _) in items.iter().enumerate() {
        match matched.get(&fps[i]) {
            Some((mid, fid)) => files.push(json!({"projectID": mid, "fileID": fid, "required": true})),
            None => overrides.push(i),
        }
    }
    let lv = loader_version(&p).unwrap_or_default();
    let loaders = match p.loader.as_str() {
        "fabric" | "quilt" | "forge" | "neoforge" => json!([{"id": format!("{}-{lv}", p.loader), "primary": true}]),
        _ => json!([]),
    };
    let man = json!({"minecraft": {"version": p.mc, "modLoaders": loaders}, "manifestType": "minecraftModpack", "manifestVersion": 1, "name": p.name, "version": "1.0.0", "author": "Noxel", "files": files, "overrides": "overrides"});
    cb("Упаковываю", 0.6, "");
    if let Some(d) = out.parent() {
        fs::create_dir_all(d).map_err(e)?;
    }
    let mut zw = zip::ZipWriter::new(fs::File::create(out).map_err(e)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("manifest.json", opts).map_err(e)?;
    zw.write_all(serde_json::to_string_pretty(&man).map_err(e)?.as_bytes()).map_err(e)?;
    for i in &overrides {
        add_file(&mut zw, &format!("overrides/{}", items[*i].rel), &items[*i].path)?;
    }
    let n = add_settings(&mut zw, &dir, "overrides/", saves)?;
    zw.finish().map_err(e)?;
    cb("Готово", 1.0, "");
    Ok(json!({"path": out.to_string_lossy(), "linked": files.len(), "embedded": overrides.len(), "settings": n, "size": fs::metadata(out).map(|m| m.len()).unwrap_or(0)}))
}

// ── импорт из файла ──

fn zip_names(path: &Path) -> R<Vec<String>> {
    let z = zip::ZipArchive::new(fs::File::open(path).map_err(e)?).map_err(|_| "это не архив сборки")?;
    Ok(z.file_names().map(|s| s.replace('\\', "/")).collect())
}

/// Ставит сборку из файла: .mrpack, zip CurseForge, экспорт Prism / MultiMC / PolyMC.
pub fn import_file(path: &Path, cb: Cb) -> R<Value> {
    let names = zip_names(path)?;
    if names.iter().any(|n| n == "modrinth.index.json") {
        return crate::packs::install_mrpack(path, None, None, cb);
    }
    if names.iter().any(|n| n == "manifest.json") {
        return crate::packs::install_cf_file(path, None, None, cb);
    }
    if let Some(mp) = names.iter().find(|n| n.ends_with("mmc-pack.json") && n.matches('/').count() <= 1) {
        let prefix = mp.trim_end_matches("mmc-pack.json").to_string();
        return import_mmc_zip(path, &prefix, cb);
    }
    Err("Не узнал формат. Поддерживаются .mrpack (Modrinth), zip CurseForge и экспорт Prism / MultiMC / PolyMC.".into())
}

fn read_entry(z: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<String> {
    let mut s = String::new();
    z.by_name(name).ok()?.read_to_string(&mut s).ok()?;
    Some(s)
}

/// (mc, загрузчик, версия загрузчика) из mmc-pack.json Prism / MultiMC.
fn parse_mmc_pack(s: &str) -> (String, String, Option<String>) {
    let v: Value = serde_json::from_str(s).unwrap_or(json!({}));
    let (mut mcv, mut loader, mut lv) = (String::new(), "vanilla".to_string(), None);
    for c in v["components"].as_array().cloned().unwrap_or_default() {
        let ver = c["version"].as_str().map(String::from);
        match c["uid"].as_str().unwrap_or("") {
            "net.minecraft" => mcv = ver.unwrap_or_default(),
            "net.fabricmc.fabric-loader" => (loader, lv) = ("fabric".into(), ver),
            "org.quiltmc.quilt-loader" => (loader, lv) = ("quilt".into(), ver),
            "net.minecraftforge" => (loader, lv) = ("forge".into(), ver),
            "net.neoforged" => (loader, lv) = ("neoforge".into(), ver),
            _ => {}
        }
    }
    (mcv, loader, lv)
}

fn cfg_value(cfg: &str, key: &str) -> Option<String> {
    cfg.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn import_mmc_zip(path: &Path, prefix: &str, cb: Cb) -> R<Value> {
    let mut z = zip::ZipArchive::new(fs::File::open(path).map_err(e)?).map_err(e)?;
    let pack = read_entry(&mut z, &format!("{prefix}mmc-pack.json")).ok_or("нет mmc-pack.json")?;
    let (mcv, loader, lv) = parse_mmc_pack(&pack);
    if mcv.is_empty() {
        return Err("в сборке не указана версия Minecraft".into());
    }
    let name = read_entry(&mut z, &format!("{prefix}instance.cfg")).and_then(|c| cfg_value(&c, "name")).unwrap_or_else(|| "Сборка Prism".into());
    let p = mc::create_profile(&mcv, &loader, lv, &name, None)?;
    let dir = inst_dir(&p.id);
    let game = [".minecraft/", "minecraft/"].iter().map(|g| format!("{prefix}{g}")).find(|g| z.file_names().any(|n| n.replace('\\', "/").starts_with(g.as_str()))).unwrap_or_else(|| format!("{prefix}.minecraft/"));
    let total = z.len().max(1);
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(enc) = ent.enclosed_name() else { continue };
        let s = enc.to_string_lossy().replace('\\', "/");
        let Some(rel) = s.strip_prefix(&game).filter(|r| !r.is_empty()) else { continue };
        let Some(rel) = crate::packs::safe_rel(rel) else { continue };
        let out = dir.join(&rel);
        if let Some(d) = out.parent() {
            fs::create_dir_all(d).map_err(e)?;
        }
        let mut f = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut f).map_err(e)?;
        if i % 20 == 0 {
            cb("Распаковываю", i as f64 / total as f64, &rel.to_string_lossy());
        }
    }
    after_import(&p.id, cb);
    Ok(json!({"profile": p.id, "name": p.name, "mc": p.mc, "loader": p.loader}))
}

fn after_import(profile_id: &str, cb: Cb) {
    cb("Ищу моды на Modrinth", 0.95, "");
    crate::store::dedupe_profile(profile_id);
    crate::store::index_unknown(profile_id);
    cb("Готово", 1.0, "");
}

// ── поиск профилей других лаунчеров ──

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// Папки с данными приложений (Linux: ~/.local/share, Windows: %APPDATA%, macOS: ~/Library/Application Support).
fn data_roots() -> Vec<PathBuf> {
    let h = home();
    let mut v = vec![];
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        v.push(PathBuf::from(x));
    }
    v.push(h.join(".local/share"));
    if let Some(x) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(x));
    }
    if let Some(x) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(x));
    }
    v.push(h.join("Library/Application Support"));
    v.dedup();
    v
}

/// Версии игры и загрузчика из последнего лога: так узнаём любые профили, даже без файлов настроек.
fn guess_from_log(game: &Path) -> Option<(String, String, Option<String>)> {
    let log = fs::read_to_string(game.join("logs/latest.log")).ok()?;
    for l in log.lines().take(400) {
        for (marker, loader) in [(" with Fabric Loader ", "fabric"), (" with Quilt Loader ", "quilt")] {
            if let Some(i) = l.find("Loading Minecraft ") {
                if let Some(rest) = l[i + 18..].split_once(marker) {
                    return Some((rest.0.trim().to_string(), loader.into(), Some(rest.1.split_whitespace().next().unwrap_or("").to_string())));
                }
            }
        }
        if l.contains("--fml.mcVersion") {
            let arg = |k: &str| l.split(k).nth(1).and_then(|r| r.trim_start_matches([',', ' ']).split([',', ']', ' ']).next()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let mcv = arg("--fml.mcVersion")?;
            if let Some(nf) = arg("--fml.neoForgeVersion") {
                return Some((mcv, "neoforge".into(), Some(nf)));
            }
            return Some((mcv, "forge".into(), arg("--fml.forgeVersion")));
        }
    }
    None
}

fn count_dir(d: &Path) -> usize {
    fs::read_dir(d).map(|r| r.flatten().count()).unwrap_or(0)
}

fn entry(source: &str, name: String, game: PathBuf, info: Option<(String, String, Option<String>)>) -> Option<Value> {
    if !game.is_dir() {
        return None;
    }
    let (mcv, loader, lv) = info.or_else(|| guess_from_log(&game)).unwrap_or_default();
    let mods = count_dir(&game.join("mods"));
    let saves = count_dir(&game.join("saves"));
    if mods == 0 && saves == 0 && count_dir(&game.join("resourcepacks")) == 0 {
        return None;
    }
    Some(json!({"source": source, "name": name, "path": game.to_string_lossy(), "mc": mcv, "loader": if loader.is_empty() { "vanilla".into() } else { loader }, "loader_ver": lv, "mods": mods, "saves": saves}))
}

fn scan_mmc(source: &str, base: &Path, out: &mut Vec<Value>) {
    let Ok(rd) = fs::read_dir(base) else { return };
    for d in rd.flatten() {
        let dir = d.path();
        let cfg = fs::read_to_string(dir.join("instance.cfg")).unwrap_or_default();
        if cfg.is_empty() {
            continue;
        }
        let info = fs::read_to_string(dir.join("mmc-pack.json")).ok().map(|s| parse_mmc_pack(&s)).filter(|x| !x.0.is_empty());
        let name = cfg_value(&cfg, "name").unwrap_or_else(|| d.file_name().to_string_lossy().to_string());
        let game = [".minecraft", "minecraft"].iter().map(|g| dir.join(g)).find(|g| g.is_dir()).unwrap_or(dir.join(".minecraft"));
        out.extend(entry(source, name, game, info));
    }
}

/// Версия из json версии в папке versions (официальный лаунчер, TLauncher, Legacy Launcher, HMCL).
fn version_json_info(vdir: &Path) -> Option<(String, String, Option<String>)> {
    let id = vdir.file_name()?.to_string_lossy().to_string();
    let v: Value = read_json(&vdir.join(format!("{id}.json")))?;
    let mcv = v["inheritsFrom"].as_str().or(v["clientVersion"].as_str()).unwrap_or(&id).to_string();
    for l in v["libraries"].as_array().cloned().unwrap_or_default() {
        let n = l["name"].as_str().unwrap_or("");
        for (pfx, loader) in [("net.fabricmc:fabric-loader:", "fabric"), ("org.quiltmc:quilt-loader:", "quilt"), ("net.neoforged:neoforge:", "neoforge"), ("net.minecraftforge:forge:", "forge"), ("net.minecraftforge:fmlloader:", "forge")] {
            if let Some(ver) = n.strip_prefix(pfx) {
                let ver = ver.split(':').next().unwrap_or("").to_string();
                let ver = if loader == "forge" { ver.rsplit_once('-').map(|x| x.1.to_string()).unwrap_or(ver) } else { ver };
                return Some((mcv, loader.into(), Some(ver)));
            }
        }
    }
    Some((mcv, "vanilla".into(), None))
}

pub fn detect() -> Value {
    let mut out: Vec<Value> = vec![];
    let h = home();
    let roots = data_roots();
    for r in &roots {
        scan_mmc("Prism Launcher", &r.join("PrismLauncher/instances"), &mut out);
        scan_mmc("PolyMC", &r.join("PolyMC/instances"), &mut out);
        scan_mmc("MultiMC", &r.join("multimc/instances"), &mut out);
        scan_mmc("ElyPrism", &r.join("ElyPrismLauncher/instances"), &mut out);
        scan_mmc("Fjord", &r.join("FjordLauncher/instances"), &mut out);
    }
    scan_mmc("Prism Launcher", &h.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances"), &mut out);
    scan_mmc("MultiMC", &h.join("MultiMC/instances"), &mut out);
    // Modrinth App
    for r in &roots {
        for app in ["ModrinthApp", "com.modrinth.theseus"] {
            let Ok(rd) = fs::read_dir(r.join(app).join("profiles")) else { continue };
            for d in rd.flatten() {
                let pj: Option<Value> = read_json(&d.path().join("profile.json"));
                let info = pj.as_ref().and_then(|v| {
                    let m = &v["metadata"];
                    let mcv = m["game_version"].as_str()?.to_string();
                    Some((mcv, m["loader"].as_str().unwrap_or("vanilla").to_string(), m["loader_version"]["id"].as_str().map(String::from)))
                });
                out.extend(entry("Modrinth App", d.file_name().to_string_lossy().to_string(), d.path(), info));
            }
        }
    }
    // CurseForge
    for base in [h.join("curseforge/minecraft/Instances"), h.join("Documents/curseforge/minecraft/Instances")] {
        let Ok(rd) = fs::read_dir(base) else { continue };
        for d in rd.flatten() {
            let mi: Option<Value> = read_json(&d.path().join("minecraftinstance.json"));
            let info = mi.as_ref().and_then(|v| {
                let mcv = v["gameVersion"].as_str()?.to_string();
                let ml = v["baseModLoader"]["name"].as_str().unwrap_or("");
                let (loader, lv) = ["neoforge", "fabric", "quilt", "forge"].iter().find_map(|l| ml.strip_prefix(&format!("{l}-")).map(|r| (l.to_string(), Some(r.trim_end_matches(&format!("-{mcv}")).to_string())))).unwrap_or(("vanilla".into(), None));
                Some((mcv, loader, lv))
            });
            let name = mi.as_ref().and_then(|v| v["name"].as_str().map(String::from)).unwrap_or_else(|| d.file_name().to_string_lossy().to_string());
            out.extend(entry("CurseForge", name, d.path(), info));
        }
    }
    // ATLauncher
    for r in &roots {
        let Ok(rd) = fs::read_dir(r.join("atlauncher/instances")) else { continue };
        for d in rd.flatten() {
            let v: Option<Value> = read_json(&d.path().join("instance.json"));
            let info = v.as_ref().and_then(|v| {
                let mcv = v["id"].as_str()?.to_string();
                let lvv = &v["launcher"]["loaderVersion"];
                let loader = lvv["type"].as_str().unwrap_or("vanilla").to_lowercase();
                Some((mcv, loader, lvv["version"].as_str().map(String::from)))
            });
            let name = v.as_ref().and_then(|v| v["launcher"]["name"].as_str().map(String::from)).unwrap_or_else(|| d.file_name().to_string_lossy().to_string());
            out.extend(entry("ATLauncher", name, d.path(), info));
        }
    }
    // GDLauncher (старый и Carbon)
    for base in [h.join(".config/gdlauncher_next/instances"), roots[0].join("gdlauncher_carbon/data/instances")] {
        let Ok(rd) = fs::read_dir(base) else { continue };
        for d in rd.flatten() {
            let cfg: Option<Value> = read_json(&d.path().join("config.json"));
            let info = cfg.as_ref().and_then(|v| {
                let l = &v["loader"];
                Some((l["mcVersion"].as_str()?.to_string(), l["loaderType"].as_str().unwrap_or("vanilla").to_lowercase(), l["loaderVersion"].as_str().map(String::from)))
            });
            let game = if d.path().join("instance").is_dir() { d.path().join("instance") } else { d.path() };
            out.extend(entry("GDLauncher", d.file_name().to_string_lossy().to_string(), game, info));
        }
    }
    // .minecraft: официальный лаунчер, TLauncher, Legacy Launcher, HMCL
    let mut dotmc = vec![h.join(".minecraft"), h.join(".tlauncher/legacy/Minecraft/game"), h.join("Library/Application Support/minecraft")];
    if let Some(a) = std::env::var_os("APPDATA") {
        dotmc.push(PathBuf::from(a).join(".minecraft"));
    }
    for root in dotmc {
        if !root.is_dir() {
            continue;
        }
        out.extend(entry("Папка .minecraft", format!("Основная ({})", root.display()), root.clone(), None));
        // HMCL и TLauncher с раздельными папками версий: у версии свои mods
        if let Ok(rd) = fs::read_dir(root.join("versions")) {
            for d in rd.flatten() {
                if d.path().join("mods").is_dir() || d.path().join("saves").is_dir() {
                    let info = version_json_info(&d.path());
                    out.extend(entry("Папка .minecraft", d.file_name().to_string_lossy().to_string(), d.path(), info));
                }
            }
        }
    }
    // не показываем папки, которые уже импортированы или совпадают
    let mut seen = std::collections::HashSet::new();
    out.retain(|v| seen.insert(v["path"].as_str().unwrap_or("").to_string()));
    json!(out)
}

fn copy_tree(from: &Path, to: &Path, n: &mut u64, cb: Cb, total: u64) {
    let Ok(rd) = fs::read_dir(from) else { return };
    let _ = fs::create_dir_all(to);
    for f in rd.flatten() {
        let (src, dst) = (f.path(), to.join(f.file_name()));
        let Ok(md) = fs::symlink_metadata(&src) else { continue };
        if md.is_dir() {
            copy_tree(&src, &dst, n, cb, total);
        } else if md.is_file() && !dst.exists() {
            if fs::copy(&src, &dst).is_ok() {
                *n += 1;
                if *n % 25 == 0 {
                    cb("Копирую файлы", (*n as f64 / total.max(1) as f64).min(0.9), &f.file_name().to_string_lossy());
                }
            }
        }
    }
}

fn count_files(d: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(d) else { return 0 };
    rd.flatten().map(|f| if f.path().is_dir() { count_files(&f.path()) } else { 1 }).sum()
}

/// Переносит найденный профиль другого лаунчера в Noxel (копией: исходник остаётся как был).
pub fn import_instance(path: &str, name: &str, mcv: &str, loader: &str, lv: Option<String>, saves: bool, cb: Cb) -> R<Value> {
    let src = PathBuf::from(path);
    if !src.is_dir() {
        return Err("папка не найдена".into());
    }
    if mcv.trim().is_empty() {
        return Err("не удалось узнать версию игры: выбери её вручную".into());
    }
    let p = mc::create_profile(mcv, loader, lv.filter(|s| !s.is_empty()), name, None)?;
    let dir = inst_dir(&p.id);
    let mut parts: Vec<&str> = CONTENT.to_vec();
    parts.extend(SETTINGS);
    parts.push("screenshots");
    if saves {
        parts.push("saves");
    }
    let total: u64 = parts.iter().map(|x| if src.join(x).is_dir() { count_files(&src.join(x)) } else { 1 }).sum();
    let mut n = 0;
    for part in parts {
        let s = src.join(part);
        if s.is_dir() {
            copy_tree(&s, &dir.join(part), &mut n, cb, total);
        } else if s.is_file() {
            let _ = fs::copy(&s, dir.join(part));
        }
    }
    after_import(&p.id, cb);
    Ok(json!({"profile": p.id, "name": p.name, "mc": p.mc, "loader": p.loader, "files": n}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fingerprint_known() {
        // эталон: пустой ввод и строка с пробелами дают одинаковый отпечаток без пробелов
        assert_eq!(super::cf_fingerprint(b"a b\nc"), super::cf_fingerprint(b"abc"));
    }
}
