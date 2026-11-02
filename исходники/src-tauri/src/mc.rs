use crate::accounts::Account;
use crate::util::*;
use md5::{Digest as _, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// Колбэк прогресса: (этап, доля 0..1 или -1 если неизвестно, подробность).
pub type Cb<'a> = &'a (dyn Fn(&str, f64, &str) + Sync);

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const JAVA_ALL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

pub fn root() -> PathBuf {
    data_dir().join("minecraft")
}

pub fn inst_dir(id: &str) -> PathBuf {
    data_dir().join("instances").join(id)
}

// ── профили ──

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub mc: String,
    pub loader: String,
    #[serde(default)]
    pub loader_ver: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
    #[serde(default)]
    pub preset: Option<String>,
}

static PROF_LOCK: Mutex<()> = Mutex::new(());

fn profiles_path() -> PathBuf {
    data_dir().join("profiles.json")
}

pub fn load_profiles() -> Vec<Profile> {
    read_json(&profiles_path()).unwrap_or_default()
}

fn save_profiles(p: &[Profile]) {
    write_json(&profiles_path(), &p);
}

pub fn find_profile(id: &str) -> R<Profile> {
    load_profiles().into_iter().find(|p| p.id == id).ok_or_else(|| "профиль не найден".to_string())
}

fn update_profile(id: &str, f: impl FnOnce(&mut Profile)) {
    let _g = PROF_LOCK.lock().unwrap();
    let mut all = load_profiles();
    if let Some(p) = all.iter_mut().find(|p| p.id == id) {
        f(p);
        save_profiles(&all);
    }
}

pub fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            o.extend(c.to_lowercase());
        } else if !o.ends_with('-') && !o.is_empty() {
            o.push('-');
        }
    }
    let o = o.trim_matches('-').to_string();
    if o.is_empty() { "profile".into() } else { o }
}

pub fn loader_title(l: &str) -> &'static str {
    match l {
        "fabric" => "Fabric",
        "quilt" => "Quilt",
        "forge" => "Forge",
        "neoforge" => "NeoForge",
        "optifine" => "OptiFine",
        _ => "Vanilla",
    }
}

pub fn create_profile(mc: &str, loader: &str, loader_ver: Option<String>, name: &str, preset: Option<String>) -> R<Profile> {
    if !["vanilla", "fabric", "quilt", "forge", "neoforge", "optifine"].contains(&loader) {
        return Err("неизвестный загрузчик".into());
    }
    let name = if name.trim().is_empty() {
        if loader == "vanilla" { mc.to_string() } else { format!("{mc} {}", loader_title(loader)) }
    } else {
        name.trim().to_string()
    };
    let _g = PROF_LOCK.lock().unwrap();
    let mut all = load_profiles();
    let base = slug(&name);
    let mut id = base.clone();
    let mut n = 2;
    while all.iter().any(|p| p.id == id) || inst_dir(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let p = Profile {
        id: id.clone(),
        name,
        mc: mc.to_string(),
        loader: loader.to_string(),
        loader_ver,
        version_id: if loader == "vanilla" { Some(mc.to_string()) } else { None },
        created: now(),
        last_played: None,
        preset,
    };
    let dir = inst_dir(&id);
    fs::create_dir_all(dir.join("mods")).map_err(e)?;
    all.push(p.clone());
    save_profiles(&all);
    Ok(p)
}

fn copy_dir(from: &Path, to: &Path) -> R<()> {
    fs::create_dir_all(to).map_err(e)?;
    for f in fs::read_dir(from).map_err(e)?.flatten() {
        let (src, dst) = (f.path(), to.join(f.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(e)?;
        }
    }
    Ok(())
}

/// Копия профиля вместе с модами, мирами и настройками.
pub fn duplicate_profile(id: &str) -> R<Profile> {
    let src = find_profile(id)?;
    let p = create_profile(&src.mc, &src.loader, src.loader_ver.clone(), &format!("{} копия", src.name), src.preset.clone())?;
    copy_dir(&inst_dir(id), &inst_dir(&p.id))?;
    update_profile(&p.id, |x| x.version_id = src.version_id.clone());
    find_profile(&p.id)
}

pub fn delete_profile(id: &str, remove_files: bool) -> R<()> {
    let _g = PROF_LOCK.lock().unwrap();
    let mut all = load_profiles();
    all.retain(|p| p.id != id);
    save_profiles(&all);
    if remove_files {
        safe_name(id)?;
        let _ = fs::remove_dir_all(inst_dir(id));
    }
    Ok(())
}

pub fn profile_view(p: &Profile) -> Value {
    let dir = inst_dir(&p.id);
    let mods = fs::read_dir(dir.join("mods"))
        .map(|r| {
            r.flatten()
                .filter(|f| {
                    let n = f.file_name().to_string_lossy().to_string();
                    n.ends_with(".jar") || n.ends_with(".jar.disabled")
                })
                .count()
        })
        .unwrap_or(0);
    let vid = p.version_id.clone().unwrap_or_default();
    let installed = !vid.is_empty()
        && root().join("versions").join(&vid).join(format!("{vid}.json")).exists()
        && root().join("versions").join(&p.mc).join(format!("{}.jar", p.mc)).exists();
    let mut v = serde_json::to_value(p).unwrap_or(json!({}));
    v["installed"] = json!(installed);
    v["mods"] = json!(mods);
    v["dir"] = json!(dir.to_string_lossy());
    v
}

// ── версии и загрузчики ──

pub fn manifest() -> R<Value> {
    let cache = data_dir().join("cache/version_manifest.json");
    let fresh = fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < 600)
        .unwrap_or(false);
    if fresh {
        if let Some(v) = read_json::<Value>(&cache) {
            return Ok(v);
        }
    }
    match get_json(MANIFEST) {
        Ok(v) => {
            write_json(&cache, &v);
            Ok(v)
        }
        Err(x) => read_json(&cache).ok_or(x),
    }
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn ver_key(s: &str) -> Vec<u64> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse().unwrap_or(0));
    }
    out
}

/// Список версий загрузчика для данной версии Minecraft (пусто — не поддерживается).
pub fn loader_versions(mc: &str, loader: &str) -> Vec<String> {
    match loader {
        "fabric" | "quilt" => {
            let base = if loader == "fabric" { "https://meta.fabricmc.net/v2" } else { "https://meta.quiltmc.org/v3" };
            let Ok(v) = get_json(&format!("{base}/versions/loader/{}", enc(mc))) else { return vec![] };
            let Some(a) = v.as_array() else { return vec![] };
            let stable: Vec<String> = a
                .iter()
                .filter(|x| x["loader"]["stable"].as_bool().unwrap_or(false))
                .filter_map(|x| x["loader"]["version"].as_str().map(String::from))
                .collect();
            let all: Vec<String> = a.iter().filter_map(|x| x["loader"]["version"].as_str().map(String::from)).collect();
            let mut out = stable;
            for x in all {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
            out.truncate(40);
            out
        }
        "forge" => {
            let Ok(v) = get_json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json") else {
                return vec![];
            };
            let mut out = vec![];
            for k in ["recommended", "latest"] {
                if let Some(s) = v["promos"][format!("{mc}-{k}")].as_str() {
                    if !out.contains(&s.to_string()) {
                        out.push(s.to_string());
                    }
                }
            }
            out
        }
        "neoforge" => {
            let prefix = match mc.strip_prefix("1.") {
                Some(rest) => {
                    let mut it = rest.split('.');
                    let major = it.next().unwrap_or("");
                    let minor = it.next().unwrap_or("0");
                    format!("{major}.{minor}.")
                }
                None => format!("{mc}."),
            };
            let Ok(v) = get_json("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge") else {
                return vec![];
            };
            let mut list: Vec<String> = v["versions"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).filter(|x| x.starts_with(&prefix)).map(String::from).collect())
                .unwrap_or_default();
            list.sort_by(|a, b| {
                let (pa, pb) = (a.contains('-'), b.contains('-'));
                pa.cmp(&pb).then_with(|| ver_key(b).cmp(&ver_key(a)))
            });
            list.truncate(40);
            list
        }
        "optifine" => {
            let Ok(v) = get_json(&format!("https://bmclapi2.bangbang93.com/optifine/{}", enc(mc))) else { return vec![] };
            let mut list: Vec<String> = v
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|x| !x["filename"].as_str().unwrap_or("").starts_with("preview_"))
                        .filter_map(|x| Some(format!("{}_{}", x["type"].as_str()?, x["patch"].as_str()?)))
                        .collect()
                })
                .unwrap_or_default();
            list.sort_by(|a, b| b.cmp(a));
            list.dedup();
            list
        }
        _ => vec![],
    }
}

// ── версия: JSON, наследование, правила ──

fn version_json_path(id: &str) -> PathBuf {
    root().join("versions").join(id).join(format!("{id}.json"))
}

fn ensure_vanilla_json(mc: &str) -> R<()> {
    let p = version_json_path(mc);
    if p.exists() {
        return Ok(());
    }
    let m = manifest()?;
    let ent = m["versions"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["id"] == mc))
        .ok_or_else(|| format!("версия {mc} не найдена"))?;
    download(ent["url"].as_str().unwrap_or(""), &p, ent["sha1"].as_str())
}

fn lib_key(l: &Value) -> String {
    let n = l["name"].as_str().unwrap_or("");
    let p: Vec<&str> = n.split('@').next().unwrap_or("").split(':').collect();
    if p.len() >= 4 { format!("{}:{}:{}", p[0], p[1], p[3]) } else if p.len() >= 2 { format!("{}:{}", p[0], p[1]) } else { n.to_string() }
}

fn merge(parent: Value, child: Value) -> Value {
    let mut out = parent;
    let base = out["_base"].clone();
    let Some(co) = child.as_object() else { return out };
    for (k, v) in co {
        match k.as_str() {
            "inheritsFrom" => {}
            "libraries" => {
                let mut libs: Vec<Value> = v.as_array().cloned().unwrap_or_default();
                let names: HashSet<String> = libs.iter().map(lib_key).collect();
                if let Some(pl) = out["libraries"].as_array() {
                    libs.extend(pl.iter().filter(|l| !names.contains(&lib_key(l))).cloned());
                }
                out["libraries"] = Value::Array(libs);
            }
            "arguments" => {
                for sect in ["game", "jvm"] {
                    let mut a = out["arguments"][sect].as_array().cloned().unwrap_or_default();
                    if let Some(ca) = v[sect].as_array() {
                        a.extend(ca.iter().cloned());
                    }
                    out["arguments"][sect] = Value::Array(a);
                }
            }
            _ => out[k.as_str()] = v.clone(),
        }
    }
    out["_base"] = base;
    out
}

fn load_version(id: &str) -> R<Value> {
    ensure_vanilla_json(id).ok();
    let p = version_json_path(id);
    let mut v: Value = read_json(&p).ok_or_else(|| format!("не удалось прочитать {id}.json"))?;
    if let Some(parent) = v["inheritsFrom"].as_str().map(String::from) {
        let pv = load_version(&parent)?;
        Ok(merge(pv, v))
    } else {
        v["_base"] = json!(id);
        Ok(v)
    }
}

fn arch_ok(a: &str) -> bool {
    match a {
        "x86_64" | "amd64" => cfg!(target_arch = "x86_64"),
        "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
        _ => false,
    }
}

fn rule_matches(r: &Value, feats: &HashSet<&str>) -> bool {
    if let Some(os) = r.get("os") {
        if let Some(n) = os["name"].as_str() {
            if n != OS_NAME {
                return false;
            }
        }
        if let Some(a) = os["arch"].as_str() {
            if !arch_ok(a) {
                return false;
            }
        }
        if os["version"].as_str().is_some() {
            return false;
        }
    }
    if let Some(f) = r["features"].as_object() {
        for (k, want) in f {
            if feats.contains(k.as_str()) != want.as_bool().unwrap_or(true) {
                return false;
            }
        }
    }
    true
}

fn rules_ok(rules: &Value, feats: &HashSet<&str>) -> bool {
    let Some(a) = rules.as_array() else { return true };
    if a.is_empty() {
        return true;
    }
    let mut allow = false;
    for r in a {
        if rule_matches(r, feats) {
            allow = r["action"] == "allow";
        }
    }
    allow
}

fn maven_path(name: &str) -> String {
    let (n, ext) = name.split_once('@').unwrap_or((name, "jar"));
    let p: Vec<&str> = n.split(':').collect();
    if p.len() < 3 {
        return n.replace(':', "/");
    }
    let cl = p.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    format!("{}/{}/{}/{}-{}{cl}.{ext}", p[0].replace('.', "/"), p[1], p[2], p[1], p[2])
}

struct LibFile {
    path: PathBuf,
    url: String,
    sha1: Option<String>,
    size: u64,
}

struct Libs {
    classpath: Vec<LibFile>,
    natives: Vec<(LibFile, Vec<String>)>,
}

fn lib_files(v: &Value, feats: &HashSet<&str>) -> Libs {
    let libdir = root().join("libraries");
    let mut out = Libs { classpath: vec![], natives: vec![] };
    let mut seen = HashSet::new();
    for lib in v["libraries"].as_array().cloned().unwrap_or_default() {
        if !rules_ok(&lib["rules"], feats) {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");
        let art = &lib["downloads"]["artifact"];
        let has_natives = lib["natives"][OS_NAME].is_string();
        if let Some(path) = art["path"].as_str() {
            out.classpath.push(LibFile {
                path: libdir.join(path),
                url: art["url"].as_str().unwrap_or("").to_string(),
                sha1: art["sha1"].as_str().map(String::from),
                size: art["size"].as_u64().unwrap_or(0),
            });
        } else if !has_natives && !name.is_empty() {
            let path = maven_path(name);
            let base = lib["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
            let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
            out.classpath.push(LibFile { path: libdir.join(&path), url: format!("{base}{path}"), sha1: None, size: 0 });
        }
        if let Some(cl) = lib["natives"][OS_NAME].as_str() {
            let cl = cl.replace("${arch}", "64");
            let c = &lib["downloads"]["classifiers"][&cl];
            if let Some(path) = c["path"].as_str() {
                let excl: Vec<String> = lib["extract"]["exclude"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                out.natives.push((
                    LibFile {
                        path: libdir.join(path),
                        url: c["url"].as_str().unwrap_or("").to_string(),
                        sha1: c["sha1"].as_str().map(String::from),
                        size: c["size"].as_u64().unwrap_or(0),
                    },
                    excl,
                ));
            }
        }
    }
    out.classpath.retain(|l| seen.insert(l.path.clone()));
    out
}

fn need_download(f: &LibFile) -> bool {
    match fs::metadata(&f.path) {
        Ok(m) => f.size > 0 && m.len() != f.size,
        Err(_) => true,
    }
}

// ── Java ──

fn ensure_java(component: &str, cb: Cb) -> R<PathBuf> {
    let dir = data_dir().join("runtime").join(component);
    let bin = dir.join(JAVA_EXE);
    let marker = dir.join(".ok");
    if bin.exists() && marker.exists() {
        return Ok(bin);
    }
    cb("Java", -1.0, component);
    let all = get_json(JAVA_ALL)?;
    let murl = all[JAVA_KEY][component][0]["manifest"]["url"]
        .as_str()
        .ok_or_else(|| format!("для Java {component} нет сборки под этот ПК: укажи свою Java в настройках"))?;
    let man = get_json(murl)?;
    let files = man["files"].as_object().ok_or("пустой манифест Java")?;
    let mut jobs = vec![];
    let mut exec = vec![];
    for (k, f) in files {
        let p = dir.join(k);
        match f["type"].as_str() {
            Some("directory") => {
                let _ = fs::create_dir_all(&p);
            }
            Some("file") => {
                let raw = &f["downloads"]["raw"];
                if f["executable"].as_bool().unwrap_or(false) {
                    exec.push(p.clone());
                }
                let size = raw["size"].as_u64().unwrap_or(0);
                if fs::metadata(&p).map(|m| m.len() == size).unwrap_or(false) {
                    continue;
                }
                jobs.push(Job {
                    url: raw["url"].as_str().unwrap_or("").to_string(),
                    path: p,
                    sha1: raw["sha1"].as_str().map(String::from),
                });
            }
            _ => {}
        }
    }
    let total = jobs.len();
    let label = format!("Java {}", all[JAVA_KEY][component][0]["version"]["name"].as_str().unwrap_or(component));
    download_all(jobs, 8, &|d, t| cb("Java", d as f64 / t as f64, &format!("{label} · {d}/{t}")))?;
    let _ = total;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in exec {
            let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
        }
    }
    fs::write(&marker, "ok").map_err(e)?;
    Ok(bin)
}

// ── установка ──

fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> R<()> {
    let f = fs::File::open(jar).map_err(e)?;
    let mut z = zip::ZipArchive::new(f).map_err(e)?;
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(e)?;
        if ent.is_dir() {
            continue;
        }
        let Some(rel) = ent.enclosed_name() else { continue };
        let rs = rel.to_string_lossy().to_string();
        if exclude.iter().any(|x| rs.starts_with(x.as_str())) {
            continue;
        }
        let out = dest.join(&rel);
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(e)?;
        }
        let mut o = fs::File::create(&out).map_err(e)?;
        std::io::copy(&mut ent, &mut o).map_err(e)?;
    }
    Ok(())
}

fn java_component(v: &Value) -> String {
    v["javaVersion"]["component"].as_str().unwrap_or("jre-legacy").to_string()
}

fn run_installer(java: &Path, installer: &Path, cb: Cb) -> R<()> {
    let r = root();
    fs::create_dir_all(&r).map_err(e)?;
    let lp = r.join("launcher_profiles.json");
    if !lp.exists() {
        fs::write(&lp, r#"{"profiles":{},"settings":{},"version":3}"#).map_err(e)?;
    }
    let mut child = cmd(java)
        .arg("-jar")
        .arg(installer)
        .arg("--installClient")
        .arg(&r)
        .current_dir(data_dir().join("cache"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|x| format!("не удалось запустить установщик: {x}"))?;
    let err = child.stderr.take().unwrap();
    let tail = Arc::new(Mutex::new(Vec::<String>::new()));
    let t2 = tail.clone();
    let th = std::thread::spawn(move || {
        for l in BufReader::new(err).lines().map_while(Result::ok) {
            let mut t = t2.lock().unwrap();
            t.push(l);
            if t.len() > 12 {
                t.remove(0);
            }
        }
    });
    for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let l = l.trim().to_string();
        if !l.is_empty() {
            cb("Загрузчик", -1.0, &l);
            let mut t = tail.lock().unwrap();
            t.push(l);
            if t.len() > 12 {
                t.remove(0);
            }
        }
    }
    let st = child.wait().map_err(e)?;
    let _ = th.join();
    if !st.success() {
        let t = tail.lock().unwrap().join("\n");
        if t.contains("invalid outputs") && system_zlib_is_ng() {
            return Err("Этот установщик Forge не проходит проверку на твоей системе: в ней стоит zlib-ng (CachyOS, Fedora), а Java сжимает jar-файлы с его помощью, из-за чего их контрольные суммы не совпадают с эталоном Forge. Для старых версий Forge это известная проблема. Выбери NeoForge или Fabric, либо более новую версию Minecraft.".into());
        }
        return Err(format!("установщик завершился с ошибкой:\n{t}"));
    }
    Ok(())
}

pub fn system_zlib_is_ng() -> bool {
    ["/usr/lib/libz.so.1", "/usr/lib64/libz.so.1", "/lib/x86_64-linux-gnu/libz.so.1"]
        .iter()
        .filter_map(|p| fs::read(p).ok())
        .any(|b| b.windows(7).any(|w| w == b"zlib-ng"))
}

fn versions_set() -> HashSet<String> {
    fs::read_dir(root().join("versions"))
        .map(|r| r.flatten().map(|f| f.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default()
}

fn ensure_loader(p: &Profile, cb: Cb) -> R<String> {
    if p.loader == "vanilla" {
        return Ok(p.mc.clone());
    }
    if let Some(vid) = &p.version_id {
        if version_json_path(vid).exists() {
            return Ok(vid.clone());
        }
    }
    cb("Загрузчик", -1.0, loader_title(&p.loader));
    let lv = match &p.loader_ver {
        Some(v) => v.clone(),
        None => loader_versions(&p.mc, &p.loader)
            .into_iter()
            .next()
            .ok_or_else(|| format!("{} недоступен для {}", loader_title(&p.loader), p.mc))?,
    };
    let vid = match p.loader.as_str() {
        "fabric" | "quilt" => {
            let base = if p.loader == "fabric" { "https://meta.fabricmc.net/v2" } else { "https://meta.quiltmc.org/v3" };
            let j = get_json(&format!("{base}/versions/loader/{}/{}/profile/json", enc(&p.mc), enc(&lv)))?;
            let id = j["id"].as_str().ok_or("в профиле загрузчика нет id")?.to_string();
            write_json(&version_json_path(&id), &j);
            id
        }
        "optifine" => install_optifine(&p.mc, &lv, cb)?,
        _ => {
            let vj = load_version(&p.mc)?;
            let java = ensure_java(&java_component(&vj), cb)?;
            ensure_client_jar(&vj, cb)?;
            let (url, expect) = if p.loader == "forge" {
                (
                    format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{0}-{1}/forge-{0}-{1}-installer.jar", p.mc, lv),
                    format!("{}-forge-{}", p.mc, lv),
                )
            } else {
                (
                    format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar", lv),
                    format!("neoforge-{lv}"),
                )
            };
            let installer = data_dir().join("cache").join(format!("{}-{}-installer.jar", p.loader, lv));
            if !installer.exists() {
                cb("Загрузчик", -1.0, "скачиваю установщик");
                download(&url, &installer, None)?;
            }
            let before = versions_set();
            run_installer(&java, &installer, cb)?;
            if version_json_path(&expect).exists() {
                expect
            } else {
                versions_set()
                    .difference(&before)
                    .find(|n| version_json_path(n).exists())
                    .cloned()
                    .ok_or("установщик не создал версию")?
            }
        }
    };
    update_profile(&p.id, |x| {
        x.version_id = Some(vid.clone());
        x.loader_ver = Some(lv.clone());
    });
    Ok(vid)
}

/// OptiFine без графического установщика: патчим клиент его же Patcher-ом и пишем версию.
fn install_optifine(mc: &str, lv: &str, cb: Cb) -> R<String> {
    let (typ, patch) = lv.rsplit_once('_').ok_or("неверная версия OptiFine")?;
    let ed = format!("{typ}_{patch}");
    let id = format!("{mc}-OptiFine_{ed}");
    let installer = data_dir().join("cache").join(format!("OptiFine_{mc}_{ed}.jar"));
    if !installer.exists() {
        cb("Загрузчик", -1.0, "скачиваю OptiFine");
        download(&format!("https://bmclapi2.bangbang93.com/optifine/{}/{typ}/{patch}", enc(mc)), &installer, None)?;
    }
    let vj = load_version(mc)?;
    let java = ensure_java(&java_component(&vj), cb)?;
    let base = ensure_client_jar(&vj, cb)?;
    let lib = root().join("libraries/optifine/OptiFine").join(format!("{mc}_{ed}")).join(format!("OptiFine-{mc}_{ed}.jar"));
    if !lib.exists() {
        cb("Загрузчик", -1.0, "патчу клиент");
        fs::create_dir_all(lib.parent().unwrap()).map_err(e)?;
        let out = cmd(&java)
            .arg("-cp")
            .arg(&installer)
            .arg("optifine.Patcher")
            .arg(&base)
            .arg(&installer)
            .arg(&lib)
            .output()
            .map_err(|x| format!("не удалось запустить Java: {x}"))?;
        if !out.status.success() || !lib.exists() {
            let _ = fs::remove_file(&lib);
            return Err(format!("патч OptiFine не удался:\n{}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    // launchwrapper-of лежит внутри самого установщика
    let mut z = zip::ZipArchive::new(fs::File::open(&installer).map_err(e)?).map_err(e)?;
    let lwv = {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name("launchwrapper-of.txt").map_err(e)?, &mut s).map_err(e)?;
        s.trim().to_string()
    };
    let lw = root().join("libraries/optifine/launchwrapper-of").join(&lwv).join(format!("launchwrapper-of-{lwv}.jar"));
    if !lw.exists() {
        fs::create_dir_all(lw.parent().unwrap()).map_err(e)?;
        let mut f = fs::File::create(&lw).map_err(e)?;
        std::io::copy(&mut z.by_name(&format!("launchwrapper-of-{lwv}.jar")).map_err(e)?, &mut f).map_err(e)?;
    }
    write_json(
        &version_json_path(&id),
        &json!({
            "id": id, "inheritsFrom": mc, "type": "release", "mainClass": "net.minecraft.launchwrapper.Launch",
            "arguments": {"game": ["--tweakClass", "optifine.OptiFineTweaker"]},
            "libraries": [{"name": format!("optifine:OptiFine:{mc}_{ed}")}, {"name": format!("optifine:launchwrapper-of:{lwv}")}]
        }),
    );
    Ok(id)
}

fn ensure_client_jar(v: &Value, cb: Cb) -> R<PathBuf> {
    let base = v["_base"].as_str().ok_or("нет базовой версии")?;
    let jar = root().join("versions").join(base).join(format!("{base}.jar"));
    let c = &v["downloads"]["client"];
    let size = c["size"].as_u64().unwrap_or(0);
    if fs::metadata(&jar).map(|m| size == 0 || m.len() == size).unwrap_or(false) {
        return Ok(jar);
    }
    cb("Игра", -1.0, "скачиваю client.jar");
    download(c["url"].as_str().ok_or("в версии нет client.jar")?, &jar, c["sha1"].as_str())?;
    Ok(jar)
}

/// Предзагрузка игры без профиля и запуска: клиент, библиотеки, ресурсы и Java (нужна установщику).
pub fn prefetch(version: &str, threads: usize, java_only: bool, cb: Cb) -> R<String> {
    let id = if version == "latest" {
        manifest()?["latest"]["release"].as_str().ok_or("в списке Mojang нет последнего релиза")?.to_string()
    } else {
        version.to_string()
    };
    cb("Метаданные", -1.0, &id);
    ensure_vanilla_json(&id)?;
    let v = load_version(&id)?;
    if java_only {
        ensure_java(&java_component(&v), cb)?;
        return Ok(id);
    }
    ensure_client_jar(&v, cb)?;
    let libs = lib_files(&v, &HashSet::new());
    let jobs: Vec<Job> = libs
        .classpath
        .iter()
        .chain(libs.natives.iter().map(|(f, _)| f))
        .filter(|f| need_download(f) && !f.url.is_empty())
        .map(|f| Job { url: f.url.clone(), path: f.path.clone(), sha1: f.sha1.clone() })
        .collect();
    download_all(jobs, threads, &|d, t| cb("Библиотеки", d as f64 / t as f64, &format!("{d}/{t}")))?;
    let assets_root = root().join("assets");
    let ai = &v["assetIndex"];
    let ai_id = ai["id"].as_str().or(v["assets"].as_str()).unwrap_or("legacy").to_string();
    let idx_path = assets_root.join("indexes").join(format!("{ai_id}.json"));
    if !idx_path.exists() {
        download(ai["url"].as_str().ok_or("в версии нет индекса ресурсов")?, &idx_path, ai["sha1"].as_str())?;
    }
    let idx: Value = read_json(&idx_path).ok_or("битый индекс ресурсов")?;
    let mut jobs = vec![];
    for (_, o) in idx["objects"].as_object().cloned().unwrap_or_default() {
        let h = o["hash"].as_str().unwrap_or("");
        if h.len() < 2 {
            continue;
        }
        let path = assets_root.join("objects").join(&h[..2]).join(h);
        if fs::metadata(&path).map(|m| m.len() == o["size"].as_u64().unwrap_or(0)).unwrap_or(false) {
            continue;
        }
        jobs.push(Job { url: format!("https://resources.download.minecraft.net/{}/{h}", &h[..2]), path, sha1: Some(h.to_string()) });
    }
    download_all(jobs, threads, &|d, t| cb("Ресурсы", d as f64 / t as f64, &format!("{d}/{t}")))?;
    ensure_java(&java_component(&v), cb)?;
    Ok(id)
}

/// сервер, на который надо зайти сразу после запуска (ставит лаунчер, забирает prepare)
pub static JOIN: Mutex<Option<String>> = Mutex::new(None);
/// профиль запущенной сейчас игры (для локального API мода)
pub static RUNNING: Mutex<Option<String>> = Mutex::new(None);

pub struct Prepared {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub gamedir: PathBuf,
}

/// authlib-injector (AGPL-3.0, отдельная программа, скачивается с сайта автора и запускается как есть):
/// подменяет серверы входа Mojang на Ely.by или сервер Noxel. Кэшируется в tools/.
fn ensure_authlib_injector() -> R<PathBuf> {
    let dir = data_dir().join("tools");
    let _ = fs::create_dir_all(&dir);
    let cached = || fs::read_dir(&dir).ok().and_then(|r| r.flatten().map(|f| f.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("authlib-injector-")).unwrap_or(false)).max());
    let stamp = dir.join("authlib-injector.checked");
    let fresh = fs::metadata(&stamp).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() < 86400).unwrap_or(false);
    if let (true, Some(p)) = (fresh, cached()) {
        return Ok(p);
    }
    match get_json("https://authlib-injector.yushi.moe/artifact/latest.json") {
        Ok(v) => {
            let ver = v["version"].as_str().unwrap_or("latest");
            let url = v["download_url"].as_str().ok_or("нет ссылки на authlib-injector")?;
            let path = dir.join(format!("authlib-injector-{ver}.jar"));
            if !path.exists() {
                download(url, &path, None)?;
                if let Some(want) = v["checksums"]["sha256"].as_str() {
                    use sha2::Digest;
                    let got = format!("{:x}", sha2::Sha256::digest(fs::read(&path).map_err(e)?));
                    if !got.eq_ignore_ascii_case(want) {
                        let _ = fs::remove_file(&path);
                        return Err("authlib-injector скачался повреждённым".into());
                    }
                }
            }
            let _ = fs::write(&stamp, ver);
            Ok(path)
        }
        Err(x) => cached().ok_or_else(|| format!("не удалось скачать authlib-injector: {x}")),
    }
}

pub fn prepare(p: &Profile, st: &Settings, acct: &Account, cb: Cb) -> R<Prepared> {
    let threads = st.threads.clamp(2, 24) as usize;
    // вход через authlib-injector: Ely.by и другие серверы, а по желанию и аккаунт Noxel
    let mut acct = acct.clone();
    let mut injector: Option<String> = None;
    if acct.kind == "ygg" {
        injector = Some(acct.api.clone());
    } else if acct.kind == "noxel" && st.noxel_ygg && !acct.token.is_empty() {
        match crate::accounts::noxel_game_token(&acct) {
            Ok((tok, uuid)) => {
                acct.access_token = tok;
                acct.uuid = uuid;
                injector = Some(crate::accounts::NOXEL_YGG.into());
            }
            Err(x) => eprintln!("[ygg] вход Noxel без authlib-injector: {x}"),
        }
    }
    let acct = &acct;
    cb("Метаданные", -1.0, &p.mc);
    ensure_vanilla_json(&p.mc)?;
    let vid = ensure_loader(p, cb)?;
    let v = load_version(&vid)?;
    let base = v["_base"].as_str().unwrap_or(&p.mc).to_string();
    let jar = ensure_client_jar(&v, cb)?;

    let mut feats: HashSet<&str> = HashSet::new();
    if st.width > 0 && st.height > 0 {
        feats.insert("has_custom_resolution");
    }

    // библиотеки
    let libs = lib_files(&v, &feats);
    let mut jobs = vec![];
    for f in libs.classpath.iter().chain(libs.natives.iter().map(|(f, _)| f)) {
        if need_download(f) {
            if f.url.is_empty() && !f.path.exists() {
                return Err(format!("нет файла библиотеки {}", f.path.display()));
            }
            if !f.url.is_empty() {
                jobs.push(Job { url: f.url.clone(), path: f.path.clone(), sha1: f.sha1.clone() });
            }
        }
    }
    download_all(jobs, threads, &|d, t| cb("Библиотеки", d as f64 / t as f64, &format!("{d}/{t}")))?;

    let natives_dir = root().join("versions").join(&vid).join("natives");
    let _ = fs::create_dir_all(&natives_dir);
    for (f, excl) in &libs.natives {
        extract_natives(&f.path, &natives_dir, excl)?;
    }

    // ресурсы
    let gamedir = inst_dir(&p.id);
    fs::create_dir_all(&gamedir).map_err(e)?;
    let assets_root = root().join("assets");
    let ai = &v["assetIndex"];
    let ai_id = ai["id"].as_str().or(v["assets"].as_str()).unwrap_or("legacy").to_string();
    let idx_path = assets_root.join("indexes").join(format!("{ai_id}.json"));
    if !idx_path.exists() {
        cb("Ресурсы", -1.0, "индекс");
        download(ai["url"].as_str().ok_or("в версии нет индекса ресурсов")?, &idx_path, ai["sha1"].as_str())?;
    }
    let idx: Value = read_json(&idx_path).ok_or("битый индекс ресурсов")?;
    let objects = idx["objects"].as_object().cloned().unwrap_or_default();
    let mut jobs = vec![];
    for (_, o) in &objects {
        let h = o["hash"].as_str().unwrap_or("");
        if h.len() < 2 {
            continue;
        }
        let path = assets_root.join("objects").join(&h[..2]).join(h);
        let size = o["size"].as_u64().unwrap_or(0);
        if fs::metadata(&path).map(|m| m.len() == size).unwrap_or(false) {
            continue;
        }
        jobs.push(Job {
            url: format!("https://resources.download.minecraft.net/{}/{h}", &h[..2]),
            path,
            sha1: Some(h.to_string()),
        });
    }
    download_all(jobs, threads, &|d, t| cb("Ресурсы", d as f64 / t as f64, &format!("{d}/{t}")))?;
    let mut game_assets = assets_root.clone();
    let legacy = idx["virtual"].as_bool().unwrap_or(false);
    let to_res = idx["map_to_resources"].as_bool().unwrap_or(false);
    if legacy || to_res {
        let target = if to_res { gamedir.join("resources") } else { assets_root.join("virtual").join(&ai_id) };
        for (name, o) in &objects {
            let h = o["hash"].as_str().unwrap_or("");
            if h.len() < 2 {
                continue;
            }
            let dst = target.join(name);
            if !dst.exists() {
                if let Some(d) = dst.parent() {
                    let _ = fs::create_dir_all(d);
                }
                let _ = fs::copy(assets_root.join("objects").join(&h[..2]).join(h), &dst);
            }
        }
        game_assets = target;
    }

    // конфиг логов (закрывает log4shell на старых версиях)
    let mut log_arg: Option<String> = None;
    let lc = &v["logging"]["client"];
    if let (Some(fid), Some(url)) = (lc["file"]["id"].as_str(), lc["file"]["url"].as_str()) {
        let lp = assets_root.join("log_configs").join(fid);
        if !lp.exists() {
            download(url, &lp, lc["file"]["sha1"].as_str())?;
        }
        log_arg = lc["argument"].as_str().map(|a| a.replace("${path}", &lp.to_string_lossy()));
    }

    // Java
    let java = if !st.java_path.trim().is_empty() {
        PathBuf::from(st.java_path.trim())
    } else {
        ensure_java(&java_component(&v), cb)?
    };

    // аргументы
    cb("Запуск", -1.0, "собираю команду");
    let mut cp: Vec<String> = libs.classpath.iter().map(|l| l.path.to_string_lossy().to_string()).collect();
    cp.push(jar.to_string_lossy().to_string());
    let mut vars: HashMap<&str, String> = HashMap::new();
    for (k, val) in [
        ("auth_player_name", acct.name.clone()),
        ("version_name", vid.clone()),
        ("game_directory", gamedir.to_string_lossy().to_string()),
        ("assets_root", assets_root.to_string_lossy().to_string()),
        ("assets_index_name", ai_id.clone()),
        ("auth_uuid", acct.uuid.clone()),
        ("auth_access_token", if acct.access_token.is_empty() { "0".into() } else { acct.access_token.clone() }),
        ("auth_session", "0".into()),
        ("clientid", "noxel".into()),
        ("auth_xuid", if acct.xuid.is_empty() { "0".into() } else { acct.xuid.clone() }),
        ("user_type", if acct.kind == "microsoft" { "msa".into() } else if injector.is_some() { "mojang".into() } else { "legacy".into() }),
        ("version_type", v["type"].as_str().unwrap_or("release").to_string()),
        ("user_properties", "{}".into()),
        ("natives_directory", natives_dir.to_string_lossy().to_string()),
        ("launcher_name", "Noxel".into()),
        ("launcher_version", "0.1.0".into()),
        ("classpath", cp.join(CP_SEP)),
        ("classpath_separator", CP_SEP.into()),
        ("library_directory", root().join("libraries").to_string_lossy().to_string()),
        ("game_assets", game_assets.to_string_lossy().to_string()),
        ("resolution_width", st.width.to_string()),
        ("resolution_height", st.height.to_string()),
    ] {
        vars.insert(k, val);
    }
    let sub = |s: &str| -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("${") {
            out.push_str(&rest[..i]);
            match rest[i..].find('}') {
                Some(j) => {
                    let key = &rest[i + 2..i + j];
                    match vars.get(key) {
                        Some(val) => out.push_str(val),
                        None => out.push_str(&rest[i..i + j + 1]),
                    }
                    rest = &rest[i + j + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    };
    let collect = |arr: &Value| -> Vec<String> {
        let mut out = vec![];
        for a in arr.as_array().cloned().unwrap_or_default() {
            if let Some(s) = a.as_str() {
                out.push(sub(s));
            } else if rules_ok(&a["rules"], &feats) {
                match &a["value"] {
                    Value::String(s) => out.push(sub(s)),
                    Value::Array(l) => out.extend(l.iter().filter_map(|x| x.as_str()).map(&sub)),
                    _ => {}
                }
            }
        }
        out
    };

    let mut args: Vec<String> = vec![
        format!("-Xmx{}M", st.ram_mb.max(512)),
        format!("-Xms{}M", st.ram_mb.clamp(256, 1024)),
    ];
    args.extend(st.jvm_args.split_whitespace().map(String::from));
    if let Some(api) = &injector {
        cb("Запуск", -1.0, "authlib-injector");
        let jar = ensure_authlib_injector()?;
        args.push(format!("-javaagent:{}={}", jar.display(), api));
        if let Ok(meta) = agent().get(api).call().and_then(|r| Ok(r.into_string().unwrap_or_default())) {
            use base64::Engine;
            if meta.contains("signaturePublickey") {
                args.push(format!("-Dauthlibinjector.yggdrasil.prefetched={}", base64::engine::general_purpose::STANDARD.encode(meta.as_bytes())));
            }
        }
    }
    let _ = fs::create_dir_all(gamedir.join(".noxel"));
    let _ = fs::write(gamedir.join(".noxel/flags.txt"), format!("icon={}\n", st.game_icon as u8));
    if st.game_icon {
        crate::autoauth::install_game_entry(&p.mc);
    }
    if st.auto_auth || st.auto_skin || st.game_icon {
        if let Ok(agent) = crate::autoauth::install_agent() {
            args.push(format!("-javaagent:{}={}", agent.display(), gamedir.display()));
        }
    }
    // мод Noxel (кнопка «Друзья» в игре) и адрес локального API лаунчера
    if let Some((port, key)) = crate::localapi::info() {
        args.push(format!("-Dnoxel.api=http://127.0.0.1:{port}"));
        args.push(format!("-Dnoxel.key={key}"));
    }
    if st.noxel_mod {
        crate::autoauth::install_mod(p, &gamedir);
    }
    if let Some(a) = log_arg {
        args.push(a);
    }
    let main_class = v["mainClass"].as_str().ok_or("в версии нет mainClass")?.to_string();
    let game_args: Vec<String>;
    if v["arguments"].is_object() {
        args.extend(collect(&v["arguments"]["jvm"]));
        game_args = collect(&v["arguments"]["game"]);
    } else {
        args.push(sub("-Djava.library.path=${natives_directory}"));
        args.push("-cp".into());
        args.push(cp.join(CP_SEP));
        game_args = v["minecraftArguments"].as_str().unwrap_or("").split_whitespace().map(&sub).collect();
    }
    args.push(main_class);
    args.extend(game_args);
    if st.fullscreen {
        args.push("--fullscreen".into());
    }
    if st.width > 0 && st.height > 0 && !v["arguments"].is_object() {
        args.extend(["--width".into(), st.width.to_string(), "--height".into(), st.height.to_string()]);
    }
    if let Some(host) = JOIN.lock().unwrap().take() {
        // прямой заход на сервер: с 1.20 есть quickPlay, раньше — --server
        let part = |n: usize| p.mc.split('.').nth(n).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
        if part(0) >= 26 || (part(0) == 1 && part(1) >= 20) {
            args.extend(["--quickPlayMultiplayer".into(), host]);
        } else {
            let (h, port) = host.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((host.clone(), "25565".into()));
            args.extend(["--server".into(), h, "--port".into(), port]);
        }
    }
    let _ = base;
    Ok(Prepared { java, args, gamedir })
}

pub fn offline_uuid(name: &str) -> String {
    let mut h = Md5::new();
    h.update(format!("OfflinePlayer:{name}").as_bytes());
    let mut b: [u8; 16] = h.finalize().into();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ── процесс игры ──

/// Minecraft пишет лог в XML (log4j); превращаем события в обычные строки.
#[derive(Default)]
pub struct LogFmt {
    ev: Option<(String, Vec<String>)>,
}

fn attr<'a>(line: &'a str, name: &str) -> &'a str {
    let key = format!("{name}=\"");
    match line.find(&key) {
        Some(i) => {
            let rest = &line[i + key.len()..];
            &rest[..rest.find('"').unwrap_or(rest.len())]
        }
        None => "",
    }
}

impl LogFmt {
    pub fn feed(&mut self, line: String, out: &mut Vec<String>) {
        let line = if self.ev.is_none() { line.trim_start().to_string() } else { line };
        if line.starts_with("<log4j:Event") {
            let head = format!("[{}] [{}]", attr(&line, "level"), attr(&line, "thread"));
            self.ev = Some((head, vec![]));
        } else if let Some((head, body)) = self.ev.as_mut() {
            if line.contains("</log4j:Event>") {
                let mut it = body.drain(..);
                out.push(format!("{head} {}", it.next().unwrap_or_default()));
                out.extend(it.map(|l| format!("    {l}")));
                self.ev = None;
            } else {
                let tl = line.trim_start();
                let mut t = if tl.starts_with("<log4j:") || tl.starts_with("]]>") { tl } else { line.as_str() };
                for tag in ["<log4j:Message><![CDATA[", "<log4j:Throwable><![CDATA["] {
                    t = t.strip_prefix(tag).unwrap_or(t);
                }
                for tag in ["]]></log4j:Message>", "]]></log4j:Throwable>"] {
                    t = t.strip_suffix(tag).unwrap_or(t);
                }
                if !t.trim().is_empty() {
                    body.push(t.to_string());
                }
            }
        } else {
            out.push(line);
        }
    }
}

#[derive(Default)]
pub struct Game {
    pub child: Mutex<Option<std::process::Child>>,
    pub busy: AtomicBool,
    pub logs: Mutex<Vec<String>>,
    pub running: Mutex<Option<String>>,
}

pub fn spawn_game(
    game: &Game,
    pr: &Prepared,
    emit_log: Arc<dyn Fn(Vec<String>) + Send + Sync>,
    on_line: Arc<dyn Fn(&str) + Send + Sync>,
) -> R<std::process::Child> {
    game.logs.lock().unwrap().clear();
    let mut child = cmd(&pr.java)
        .args(&pr.args)
        .current_dir(&pr.gamedir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|x| format!("не удалось запустить Java ({}): {x}", pr.java.display()))?;
    let pending = Arc::new(Mutex::new(Vec::<String>::new()));
    let alive = Arc::new(AtomicBool::new(true));
    let mut readers = vec![];
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    for r in [Box::new(out) as Box<dyn std::io::Read + Send>, Box::new(err)] {
        let pend = pending.clone();
        let on_line = on_line.clone();
        readers.push(std::thread::spawn(move || {
            let mut fmt = LogFmt::default();
            for l in BufReader::new(r).lines().map_while(|l| l.ok()) {
                let mut out = vec![];
                fmt.feed(l, &mut out);
                if !out.is_empty() {
                    for l in &out {
                        on_line(l); // мгновенная реакция (автовход), без пакетной задержки
                    }
                    pend.lock().unwrap().extend(out);
                }
            }
        }));
    }
    let (pend, al) = (pending.clone(), alive.clone());
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(150));
            let batch: Vec<String> = std::mem::take(&mut *pend.lock().unwrap());
            if !batch.is_empty() {
                emit_log(batch);
            }
            if !al.load(Ordering::Relaxed) && pend.lock().unwrap().is_empty() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for r in readers {
            let _ = r.join();
        }
        alive.store(false, Ordering::Relaxed);
    });
    Ok(child)
}
