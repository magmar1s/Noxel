//! Horizon: Minecraft Java 26.3, переписанный на Rust (github.com/horizon-rust/horizon, лицензия MIT).
//! Noxel ставит и запускает лаунчер Horizon. Вход через Microsoft и проверку, что игра куплена, делает сам Horizon,
//! мы это не обходим. Список версий приходит с сервера Noxel, поэтому новые сборки появляются у всех сами.
use crate::util::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn dir() -> PathBuf {
    data_dir().join("horizon")
}

fn plat() -> &'static str {
    if cfg!(windows) { "windows-x64" } else { "linux-x86_64" }
}

pub fn feed() -> R<Value> {
    crate::remote::horizon_feed()
}

/// Путь к установленному лаунчеру Horizon и его версия.
fn installed() -> Option<(String, PathBuf)> {
    let meta: Value = read_json(&dir().join("installed.json"))?;
    let v = meta["version"].as_str()?.to_string();
    let exe = dir().join(meta["exe"].as_str()?);
    exe.is_file().then_some((v, exe))
}

pub fn status() -> R<Value> {
    let f = feed().ok();
    let inst = installed();
    let latest = f.as_ref().and_then(|f| f["launcher"]["version"].as_str().map(String::from));
    Ok(json!({
        "supported": f.as_ref().map(|f| f["launcher"]["assets"][plat()].is_object()).unwrap_or(true),
        "installed": inst.as_ref().map(|i| i.0.clone()),
        "latest": latest,
        "update": match (&inst, &latest) { (Some(i), Some(l)) => &i.0 != l, _ => false },
        "builds": f.as_ref().map(|f| f["builds"].clone()).unwrap_or(json!([])),
        "site": "https://playhorizon.dev",
        "feed_ok": f.is_some(),
    }))
}

fn sha256_file(p: &Path) -> R<String> {
    let mut f = fs::File::open(p).map_err(e)?;
    let mut all = Vec::new();
    f.read_to_end(&mut all).map_err(e)?;
    Ok(crate::skins::sha256(&all))
}

/// Скачивает лаунчер Horizon с GitHub (адрес и sha256 из ленты Noxel), проверяет сумму и распаковывает.
pub fn install() -> R<Value> {
    let f = feed()?;
    let a = &f["launcher"]["assets"][plat()];
    let (url, sha, name) = (a["url"].as_str().ok_or("для этой системы сборки Horizon нет")?, a["sha256"].as_str().unwrap_or(""), a["name"].as_str().unwrap_or("horizon-launcher"));
    let ver = f["launcher"]["version"].as_str().ok_or("в ленте нет версии")?.to_string();
    if !url.starts_with("https://github.com/horizon-rust/horizon/releases/download/") || sha.len() != 64 {
        return Err("лента Horizon выглядит неправильно, установка остановлена".into());
    }
    let vdir = dir().join("launcher").join(&ver);
    let _ = fs::remove_dir_all(&vdir);
    fs::create_dir_all(&vdir).map_err(e)?;
    let file = vdir.join(name);
    download(url, &file, None)?;
    if !sha256_file(&file)?.eq_ignore_ascii_case(sha) {
        let _ = fs::remove_dir_all(&vdir);
        return Err("контрольная сумма скачанного файла не совпала, установка отменена".into());
    }
    let exe_rel: String;
    if cfg!(windows) {
        let mut z = zip::ZipArchive::new(fs::File::open(&file).map_err(e)?).map_err(|_| "не удалось открыть архив Horizon")?;
        for i in 0..z.len() {
            let mut ent = z.by_index(i).map_err(e)?;
            let Some(rel) = ent.enclosed_name() else { continue };
            let out = vdir.join(&rel);
            if ent.is_dir() {
                fs::create_dir_all(&out).map_err(e)?;
            } else {
                if let Some(p) = out.parent() { fs::create_dir_all(p).map_err(e)?; }
                let mut o = fs::File::create(&out).map_err(e)?;
                std::io::copy(&mut ent, &mut o).map_err(e)?;
            }
        }
        let found = walk(&vdir).into_iter().find(|p| p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("horizon-launcher.exe")).unwrap_or(false)).ok_or("в архиве нет horizon-launcher.exe")?;
        exe_rel = found.strip_prefix(dir()).map_err(e)?.to_string_lossy().replace('\\', "/");
        let _ = fs::remove_file(&file);
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o755));
        }
        exe_rel = file.strip_prefix(dir()).map_err(e)?.to_string_lossy().to_string();
    }
    write_json(&dir().join("installed.json"), &json!({"version": ver, "exe": exe_rel}));
    // старые версии лаунчера больше не нужны
    if let Ok(rd) = fs::read_dir(dir().join("launcher")) {
        for ent in rd.flatten() {
            if ent.file_name().to_string_lossy() != ver {
                let _ = fs::remove_dir_all(ent.path());
            }
        }
    }
    status()
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(d) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
        }
    }
    out
}

fn command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    // AppImage без FUSE (в части систем его нет) распаковывается во временную папку и запускается оттуда
    c.env("APPIMAGE_EXTRACT_AND_RUN", "1");
    if let Some(p) = exe.parent() { c.current_dir(p); }
    c
}

/// Окно лаунчера Horizon: здесь входят в Microsoft и выбирают версию игры.
pub fn open() -> R<()> {
    let (_, exe) = installed().ok_or("Horizon ещё не установлен")?;
    command(&exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|x| format!("не удалось запустить Horizon: {x}"))?;
    Ok(())
}

/// Запуск игры командой `horizon-launcher play`. Если Horizon отказал (не выполнен вход, игра не куплена), возвращаем его ответ.
pub fn play() -> R<()> {
    let (_, exe) = installed().ok_or("Horizon ещё не установлен")?;
    let mut ch = command(&exe).arg("play").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|x| format!("не удалось запустить Horizon: {x}"))?;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if let Some(st) = ch.try_wait().map_err(e)? {
            if st.success() { return Ok(()); }
            let mut text = String::new();
            if let Some(mut s) = ch.stderr.take() { let _ = s.read_to_string(&mut text); }
            if text.trim().is_empty() { if let Some(mut s) = ch.stdout.take() { let _ = s.read_to_string(&mut text); } }
            let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).rev().take(4).collect();
            return Err(tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // игра запущена и работает дальше сама
    std::thread::spawn(move || { let _ = ch.wait(); });
    Ok(())
}
