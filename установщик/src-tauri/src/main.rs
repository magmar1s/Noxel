#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Установщик Noxel: скачивает лаунчер с сервера, проверяет подпись автора, ставит ярлыки
//! и по желанию заранее скачивает Java и Minecraft.

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tauri::{AppHandle, Emitter};

const API: &str = "https://mr1s.site/noxel/v1";
/// Публичный ключ подписи релизов. Закрытый ключ хранится только у автора.
const RELEASE_KEY_HEX: &str = "9a2d7e4ef870ac1d75a8aed98e76c8c6588c7051ca10a13d71f62f0b0bb127b8";
const PLATFORM: &str = if cfg!(windows) { "windows-x86_64" } else { "linux-x86_64" };
const EXE: &str = if cfg!(windows) { "Noxel.exe" } else { "Noxel" };
const LOGO_PNG: &[u8] = include_bytes!("../icons/128x128@2x.png");
const LOGO_SVG: &[u8] = include_bytes!("../../ui/assets/logo.svg");

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn hide(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn default_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Programs").join("Noxel")
    } else {
        let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", std::env::var("HOME").unwrap_or_else(|_| ".".into())));
        PathBuf::from(base).join("noxel").join("app")
    }
}

fn manifest() -> R<Value> {
    let m: Value = ureq::get(&format!("{API}/launcher")).call().map_err(|x| format!("нет связи с сервером Noxel: {x}"))?.into_json().map_err(e)?;
    if m["files"][PLATFORM].is_null() {
        return Err("Для твоей системы сборка Noxel пока не опубликована.".into());
    }
    Ok(m)
}

#[tauri::command(async)]
fn plan() -> R<Value> {
    let m = manifest()?;
    let dir = default_dir();
    Ok(json!({"dir": dir.to_string_lossy(), "version": m["version"], "size": m["files"][PLATFORM]["size"], "windows": cfg!(windows), "installed": dir.join(EXE).exists(), "auto": std::env::var("NOXEL_SETUP_AUTO").ok()}))
}

#[derive(Deserialize, Clone)]
#[allow(dead_code)]
struct Opts {
    dir: String,
    menu: bool,
    desktop: bool,
    java: bool,
    game: bool,
    run: bool,
}

fn say(app: &AppHandle, stage: &str, frac: f64, detail: &str) {
    let _ = app.emit("setup", json!({"stage": stage, "frac": frac, "detail": detail}));
}

fn download(app: &AppHandle, url: &str, dest: &Path, from: f64, span: f64) -> R<()> {
    let resp = ureq::get(url).call().map_err(|x| format!("не удалось скачать: {x}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut r = resp.into_reader();
    let mut f = fs::File::create(dest).map_err(e)?;
    let (mut got, mut buf, mut last) = (0u64, [0u8; 65536], std::time::Instant::now());
    loop {
        let n = r.read(&mut buf).map_err(e)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(e)?;
        got += n as u64;
        if last.elapsed().as_millis() > 80 {
            last = std::time::Instant::now();
            let k = if total > 0 { got as f64 / total as f64 } else { 0.5 };
            say(app, "Скачиваю Noxel", from + span * k, &format!("{:.1} из {:.1} МБ", got as f64 / 1e6, total as f64 / 1e6));
        }
    }
    Ok(())
}

fn shortcuts(app: &AppHandle, exe: &Path, o: &Opts) -> R<()> {
    say(app, "Создаю ярлыки", 0.38, "");
    if cfg!(windows) {
        let ps = |target: String| -> R<()> {
            let script = format!(
                "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save()",
                target.replace('\'', "''"), exe.display().to_string().replace('\'', "''"), exe.parent().unwrap().display().to_string().replace('\'', "''")
            );
            let mut c = Command::new("powershell");
            hide(&mut c).args(["-NoProfile", "-Command", &script]).output().map_err(e)?;
            Ok(())
        };
        if o.menu {
            let dir = PathBuf::from(std::env::var("APPDATA").map_err(e)?).join("Microsoft/Windows/Start Menu/Programs");
            ps(dir.join("Noxel.lnk").display().to_string())?;
        }
        if o.desktop {
            let out = Command::new("powershell").args(["-NoProfile", "-Command", "[Environment]::GetFolderPath('Desktop')"]).output().map_err(e)?;
            let d = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !d.is_empty() {
                ps(format!("{d}\\Noxel.lnk"))?;
            }
        }
        return Ok(());
    }
    let home = std::env::var("HOME").map_err(e)?;
    let data = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let icons = PathBuf::from(&data).join("icons/hicolor");
    for (sub, bytes, name) in [("256x256/apps", LOGO_PNG, "noxel.png"), ("scalable/apps", LOGO_SVG, "noxel.svg")] {
        fs::create_dir_all(icons.join(sub)).map_err(e)?;
        fs::write(icons.join(sub).join(name), bytes).map_err(e)?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Noxel\nGenericName=Minecraft Launcher\nComment=Лаунчер Minecraft: свои моды для каждой версии\nExec=\"{}\"\nIcon=noxel\nTerminal=false\nCategories=Game;\nStartupWMClass=Noxel\n",
        exe.display()
    );
    let put = |dir: PathBuf, file: &str| -> R<()> {
        fs::create_dir_all(&dir).map_err(e)?;
        let p = dir.join(file);
        fs::write(&p, &entry).map_err(e)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
        }
        let _ = Command::new("gio").args(["set", &p.to_string_lossy(), "metadata::trusted", "true"]).output();
        Ok(())
    };
    if o.menu {
        put(PathBuf::from(&data).join("applications"), "noxel.desktop")?;
    }
    if o.desktop {
        let xdg = Command::new("xdg-user-dir").arg("DESKTOP").output().ok().map(|x| String::from_utf8_lossy(&x.stdout).trim().to_string()).filter(|s| !s.is_empty());
        let d = xdg.into_iter().chain([format!("{home}/Desktop"), format!("{home}/Рабочий стол")]).find(|p| Path::new(p).is_dir()).unwrap_or_default();
        if Path::new(&d).is_dir() {
            put(PathBuf::from(d), "Noxel.desktop")?;
        }
    }
    Ok(())
}

fn prefetch(app: &AppHandle, exe: &Path, only_java: bool) -> R<()> {
    let ver = std::env::var("NOXEL_SETUP_VERSION").unwrap_or_else(|_| "latest".into());
    let mut c = Command::new(exe);
    c.arg("--prefetch").arg(&ver);
    if only_java {
        c.arg("--only-java");
    }
    hide(&mut c);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(e)?;
    let (base, span) = if only_java { (0.45, 0.5) } else { (0.45, 0.52) };
    let mut err = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let p: Vec<&str> = line.split('\t').collect();
        match p.first().copied() {
            Some("PROGRESS") if p.len() >= 4 => {
                let f: f64 = p[2].parse().unwrap_or(-1.0);
                let stage = match p[1] { "Java" => "Скачиваю Java", "Библиотеки" => "Скачиваю библиотеки игры", "Ресурсы" => "Скачиваю ресурсы игры", "Игра" => "Скачиваю клиент игры", _ => "Готовлю игру" };
                let order = match p[1] { "Метаданные" => 0.0, "Игра" => 0.05, "Библиотеки" => 0.1, "Ресурсы" => 0.3, "Java" => 0.85, _ => 0.0 };
                let width = match p[1] { "Библиотеки" => 0.2, "Ресурсы" => 0.55, "Java" => 0.15, _ => 0.05 };
                let (o, w) = if only_java { (0.0, 1.0) } else { (order, width) };
                say(app, stage, base + span * (o + w * f.max(0.0)).min(1.0), p[3]);
            }
            Some("ERROR") => err = Some(p.get(1).unwrap_or(&"").to_string()),
            _ => {}
        }
    }
    let st = child.wait().map_err(e)?;
    if !st.success() {
        return Err(err.unwrap_or_else(|| "предзагрузка не завершилась".into()));
    }
    Ok(())
}

fn run(app: &AppHandle, o: &Opts) -> R<String> {
    say(app, "Проверяю сервер", 0.01, "");
    let m = manifest()?;
    let f = &m["files"][PLATFORM];
    let url = f["url"].as_str().ok_or("в манифесте нет ссылки")?;
    if !url.starts_with("https://") {
        return Err("ссылка на скачивание небезопасна".into());
    }
    let dir = PathBuf::from(&o.dir);
    fs::create_dir_all(&dir).map_err(|x| format!("не удалось создать папку {}: {x}", dir.display()))?;
    let part = dir.join(format!("{EXE}.part"));
    download(app, url, &part, 0.02, 0.3)?;
    say(app, "Проверяю подпись автора", 0.33, "");
    let bytes = fs::read(&part).map_err(e)?;
    let bad = |m: &str| {
        let _ = fs::remove_file(&part);
        Err(m.to_string())
    };
    if format!("{:x}", Sha256::digest(&bytes)) != f["sha256"].as_str().unwrap_or("").to_lowercase() {
        return bad("контрольная сумма скачанного файла не совпала");
    }
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, unhex(RELEASE_KEY_HEX));
    if key.verify(&bytes, &unhex(f["sig"].as_str().unwrap_or(""))).is_err() {
        return bad("подпись неверна: файл не от автора Noxel");
    }
    drop(bytes);
    let exe = dir.join(EXE);
    #[cfg(windows)]
    {
        let _ = fs::rename(&exe, exe.with_extension("old"));
    }
    fs::rename(&part, &exe).map_err(|x| format!("не удалось записать файл: {x}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&exe, fs::Permissions::from_mode(0o755));
    }
    shortcuts(app, &exe, o)?;
    if o.java || o.game {
        say(app, "Готовлю игру", 0.45, "");
        if let Err(x) = prefetch(app, &exe, !o.game) {
            // установка лаунчера удалась: предзагрузку лаунчер доделает сам при первом запуске
            say(app, "Предзагрузка пропущена", 0.97, &x);
        }
    }
    say(app, "Готово", 1.0, "");
    Ok(exe.to_string_lossy().to_string())
}

#[tauri::command]
fn start(app: AppHandle, opts: Opts) {
    std::thread::spawn(move || match run(&app, &opts) {
        Ok(exe) => {
            let _ = app.emit("setup-done", json!({"ok": true, "exe": exe}));
        }
        Err(x) => {
            let _ = app.emit("setup-done", json!({"ok": false, "error": x}));
        }
    });
}

#[tauri::command]
fn finish(app: AppHandle, run: bool, exe: String) {
    if run {
        let mut c = Command::new(&exe);
        c.current_dir(Path::new(&exe).parent().unwrap_or(Path::new(".")));
        let _ = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    app.exit(0);
}

/// Windows: окно рисует WebView2. В чистой Windows 10 его может не быть, тогда ставим с сайта Microsoft (официальный загрузчик, тихо).
#[cfg(windows)]
fn ensure_webview2() {
    use std::os::windows::process::CommandExt;
    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let has = |root: &str, path: &str| {
        std::process::Command::new("reg")
            .args(["query", &format!("{root}\\{path}\\Clients\\{GUID}"), "/v", "pv"])
            .creation_flags(0x0800_0000)
            .output()
            .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("0.0.0.0"))
            .unwrap_or(false)
    };
    if has("HKLM", "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate") || has("HKLM", "SOFTWARE\\Microsoft\\EdgeUpdate") || has("HKCU", "Software\\Microsoft\\EdgeUpdate") {
        return;
    }
    let file = std::env::temp_dir().join("noxel-webview2-setup.exe");
    let ok = ureq::get("https://go.microsoft.com/fwlink/p/?LinkId=2124703")
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .ok()
        .and_then(|r| {
            let mut f = std::fs::File::create(&file).ok()?;
            std::io::copy(&mut r.into_reader(), &mut f).ok()
        })
        .is_some();
    if ok {
        let _ = std::process::Command::new(&file).args(["/silent", "/install"]).creation_flags(0x0800_0000).status();
        let _ = std::fs::remove_file(&file);
    }
}

fn main() {
    #[cfg(windows)]
    ensure_webview2();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![plan, start, finish])
        .run(tauri::generate_context!())
        .expect("ошибка запуска установщика");
}
