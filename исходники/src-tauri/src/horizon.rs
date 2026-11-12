//! Horizon Offline Launcher: Rust-клиент Minecraft Java 26.3, интегрированный в Noxel через CLI.
//! Поддерживает офлайн-профили для одиночной игры/LAN и Microsoft-вход с проверкой владения игрой.
//! Игровые сборки и пакет клиента приходят из Noxel feed, а UI отдельного Horizon Launcher не открывается.
use crate::util::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

fn dir() -> PathBuf {
    data_dir().join("horizon")
}

fn selected_path() -> PathBuf {
    dir().join("selected-build.json")
}

fn ready_builds_path() -> PathBuf {
    dir().join("ready-builds.json")
}

fn runtime_dir() -> PathBuf { dir().join("runtime-data") }
fn game_dir() -> PathBuf { dir().join("game-data") }

fn plat() -> &'static str {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") { "windows-x64" }
    else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") { "linux-x86_64" }
    else { "unsupported" }
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
    let expected_repo = "https://github.com/gerdaroot/Horizon-Offline-Launcher";
    let latest = f.as_ref().and_then(|f| f["launcher"]["version"].as_str().map(String::from));
    let feed_ok = f.is_some();
    let mut builds = f.as_ref().and_then(|feed| feed["builds"].as_array()).cloned().unwrap_or_default();
    builds.sort_by(|a, b| b["date"].as_str().unwrap_or("").cmp(a["date"].as_str().unwrap_or("")));
    let saved_selection: Option<Value> = read_json(&selected_path());
    let selected_build = saved_selection.and_then(|v| v["tag"].as_str().map(str::to_owned))
        .filter(|tag| builds.iter().any(|b| b["tag"].as_str() == Some(tag)))
        .or_else(|| builds.first().and_then(|b| b["tag"].as_str().map(str::to_owned)));
    let ready_state: Option<Value> = read_json(&ready_builds_path());
    let mut prepared_builds = ready_state.and_then(|v| v["tags"].as_array().cloned()).unwrap_or_default();
    let mut installed_builds = inst.as_ref().and_then(|(_, exe)| {
        command(exe).arg("--json").arg("versions").arg("--installed").output().ok()
    }).filter(|out| out.status.success())
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok())
        .and_then(|v| v["installed"].as_array().cloned()).unwrap_or_default();
    if installed_builds.is_empty() { installed_builds = prepared_builds.clone(); }
    prepared_builds.retain(|tag| installed_builds.contains(tag));
    let account = inst.as_ref().and_then(|(_, exe)| {
        command(exe).arg("--json").arg("account").arg("list").output().ok()
    }).and_then(|out| {
        if !out.status.success() { return None; }
        serde_json::from_slice::<Value>(&out.stdout).ok()
    }).unwrap_or(json!({"accounts": [], "owner": false}));
    Ok(json!({
        "supported": f.as_ref().is_some_and(|f| f["repo"].as_str() == Some(expected_repo) && f["launcher"]["assets"][plat()].is_object()),
        "feed_source_ok": f.as_ref().is_some_and(|f| f["repo"].as_str() == Some(expected_repo)),
        "installed": inst.as_ref().map(|i| i.0.clone()),
        "latest": latest,
        "update": match (&inst, &latest) { (Some(i), Some(l)) => &i.0 != l, _ => false },
        "builds": builds,
        "selected_build": selected_build,
        "installed_builds": installed_builds,
        "prepared_builds": prepared_builds,
        "account": account,
        "site": "https://playhorizon.dev",
        "repository": expected_repo,
        "feed_ok": feed_ok,
    }))
}

/// Sign in through Microsoft's device-code page while the code and progress stay inside Noxel.
pub fn sign_in(app: AppHandle) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала подготовь Rust-клиент")?;
    let mut child = command(&exe).arg("--json").arg("account").arg("sign-in")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|x| format!("не удалось начать вход Microsoft: {x}"))?;
    let stdout = child.stdout.take().ok_or("не удалось прочитать ответ входа")?;
    let stderr = child.stderr.take().ok_or("не удалось прочитать ошибки входа")?;
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let out_tx = tx.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() { let _ = out_tx.send((false, line)); }
    });
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() { let _ = tx.send((true, line)); }
    });
    let mut json_block = String::new();
    let mut errors = Vec::new();
    loop {
        if let Ok((is_stderr, line)) = rx.recv_timeout(Duration::from_millis(100)) {
            if is_stderr {
                if let Some((url, code)) = parse_sign_in_code(&line) {
                    let _ = app.emit("horizon-auth", json!({"kind": "code", "url": url, "code": code}));
                } else if !line.trim().is_empty() { errors.push(line); }
                continue;
            }
            if json_block.is_empty() {
                if line.trim() != "{" { continue; }
            }
            json_block.push_str(&line);
            if let Ok(value) = serde_json::from_str::<Value>(&json_block) {
                if let (Some(url), Some(code)) = (value["link"].as_str(), value["code"].as_str()) {
                    let _ = app.emit("horizon-auth", json!({"kind": "code", "url": url, "code": code}));
                } else if let Some(uuid) = value["signed_in"].as_str() {
                    let _ = app.emit("horizon-auth", json!({"kind": "complete", "uuid": uuid}));
                }
                json_block.clear();
            } else {
                json_block.push('\n');
            }
        }
        if let Some(status) = child.try_wait().map_err(e)? {
            if status.success() { return Ok(account_status(&exe)); }
            let detail = errors.into_iter().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(if detail.is_empty() { format!("Вход Microsoft завершился с кодом {}", status.code().unwrap_or(-1)) } else { detail });
        }
    }
}

fn parse_sign_in_code(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix("To sign in, open ")?;
    let (url, rest) = rest.split_once(" in a browser and enter the code ")?;
    let code = rest.trim();
    (url.starts_with("https://") && !code.is_empty()).then(|| (url.to_owned(), code.to_owned()))
}

fn account_status(exe: &Path) -> Value {
    command(exe).arg("--json").arg("account").arg("list").output().ok()
        .filter(|out| out.status.success())
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok())
        .unwrap_or(json!({"accounts": [], "owner": false}))
}

fn known_build(tag: &str) -> R<()> {
    let f = feed()?;
    if f["builds"].as_array().is_some_and(|builds| builds.iter().any(|b| b["tag"].as_str() == Some(tag))) {
        Ok(())
    } else {
        Err("этой версии нет в каталоге Horizon Offline".into())
    }
}

pub fn select_build(tag: &str) -> R<Value> {
    known_build(tag)?;
    write_json(&selected_path(), &json!({"tag": tag}));
    status()
}

fn command_output(exe: &Path, args: &[&str]) -> R<String> {
    let out = command(exe).args(args).stdin(Stdio::null()).output().map_err(|x| format!("не удалось запустить Horizon: {x}"))?;
    if out.status.success() { return Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned()); }
    let text = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
    let output = String::from_utf8_lossy(text);
    let tail = output.lines().filter(|line| !line.trim().is_empty()).rev().take(8).collect::<Vec<_>>();
    let detail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(if detail.is_empty() { format!("Horizon завершился с кодом {}", out.status.code().unwrap_or(-1)) } else { detail })
}

pub fn install_build(tag: &str) -> R<Value> {
    known_build(tag)?;
    let (_, exe) = installed().ok_or("Сначала установи лаунчер Horizon")?;
    command_output(&exe, &["install", tag])?;
    write_json(&selected_path(), &json!({"tag": tag}));
    let prepared = command_output(&exe, &["prepare", "--version", tag]).is_ok();
    if prepared { remember_prepared(tag); }
    let mut state = status()?;
    if let Some(obj) = state.as_object_mut() { obj.insert("prepare_needed".into(), json!(!prepared)); }
    Ok(state)
}

fn remember_prepared(tag: &str) {
    let ready_state: Option<Value> = read_json(&ready_builds_path());
    let mut tags = ready_state.and_then(|v| v["tags"].as_array().cloned()).unwrap_or_default();
    if !tags.iter().any(|v| v.as_str() == Some(tag)) { tags.push(json!(tag)); }
    write_json(&ready_builds_path(), &json!({"tags": tags}));
}

fn command_output_owned(exe: &Path, args: &[String]) -> R<String> {
    let out = command(exe).args(args).stdin(Stdio::null()).output().map_err(|x| format!("не удалось запустить Horizon: {x}"))?;
    if out.status.success() { return Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned()); }
    let text = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
    let lines = String::from_utf8_lossy(text).lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let detail = lines.iter().rev().take(8).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(if detail.is_empty() { format!("Horizon завершился с кодом {}", out.status.code().unwrap_or(-1)) } else { detail })
}

pub fn prepare_build(tag: &str, minecraft: Option<&str>, download_mojang: bool) -> R<Value> {
    known_build(tag)?;
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент")?;
    let mut args = vec!["prepare".to_owned(), "--version".to_owned(), tag.to_owned()];
    if download_mojang { args.push("--download".to_owned()); }
    else if let Some(path) = minecraft.map(str::trim).filter(|p| !p.is_empty()) {
        args.push("--minecraft".to_owned()); args.push(path.to_owned());
    }
    command_output_owned(&exe, &args)?;
    remember_prepared(tag);
    status()
}

pub fn offline_name(name: Option<&str>) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент")?;
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    if let Some(name) = name {
        if name.len() < 3 || name.len() > 16 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("Ник должен содержать 3–16 латинских букв, цифр или знаков подчёркивания".into());
        }
        command_output(&exe, &["account", "offline", name])?;
    } else { command_output(&exe, &["account", "offline"])?; }
    status()
}

pub fn select_account(name: &str) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент")?;
    let name = name.trim();
    if name.is_empty() { return Err("Выбери аккаунт".into()); }
    command_output(&exe, &["account", "select", name])?;
    status()
}

pub fn sign_out(name: &str) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент")?;
    let name = name.trim();
    if name.is_empty() { return Err("Выбери аккаунт Microsoft".into()); }
    command_output(&exe, &["account", "sign-out", name])?;
    status()
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
    let expected_name = if cfg!(windows) { "horizon-launcher-windows-x64.zip" } else { "horizon-launcher-linux-x86_64.tar.gz" };
    if !url.starts_with("https://github.com/gerdaroot/Horizon-Offline-Launcher/releases/download/") || sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) || name != expected_name {
        return Err("лента Horizon Offline выглядит неправильно, установка остановлена".into());
    }
    let staging = dir().join("launcher").join(format!(".install-{ver}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(e)?;
    let file = staging.join(name);
    if let Err(error) = download(url, &file, None) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if !sha256_file(&file)?.eq_ignore_ascii_case(sha) {
        let _ = fs::remove_dir_all(&staging);
        return Err("контрольная сумма скачанного файла не совпала, установка отменена".into());
    }
    let exe_rel: String;
    if cfg!(windows) {
        let mut z = zip::ZipArchive::new(fs::File::open(&file).map_err(e)?).map_err(|_| "не удалось открыть архив Horizon")?;
        for i in 0..z.len() {
            let mut ent = z.by_index(i).map_err(e)?;
            let Some(rel) = ent.enclosed_name() else { continue };
            let out = staging.join(&rel);
            if ent.is_dir() {
                fs::create_dir_all(&out).map_err(e)?;
            } else {
                if let Some(p) = out.parent() { fs::create_dir_all(p).map_err(e)?; }
                let mut o = fs::File::create(&out).map_err(e)?;
                std::io::copy(&mut ent, &mut o).map_err(e)?;
            }
        }
        let found = walk(&staging).into_iter().find(|p| p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("horizon-launcher.exe")).unwrap_or(false)).ok_or("в архиве нет horizon-launcher.exe")?;
        let rel = found.strip_prefix(&staging).map_err(e)?.to_path_buf();
        let _ = fs::remove_file(&file);
        exe_rel = dir().join("launcher").join(&ver).join(rel).strip_prefix(&dir()).map_err(e)?.to_string_lossy().replace('\\', "/");
    } else {
        let output = Command::new("tar").arg("-xzf").arg(&file).arg("-C").arg(&staging)
            .stdin(Stdio::null()).output().map_err(|x| format!("не удалось распаковать Horizon: {x}"))?;
        if !output.status.success() {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("не удалось распаковать архив Horizon: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        let found = walk(&staging).into_iter().find(|p| p.file_name().is_some_and(|n| n == "horizon-launcher"))
            .ok_or("в архиве нет horizon-launcher")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&found, fs::Permissions::from_mode(0o755));
        }
        let rel = found.strip_prefix(&staging).map_err(e)?.to_path_buf();
        let _ = fs::remove_file(&file);
        exe_rel = dir().join("launcher").join(&ver).join(rel).strip_prefix(&dir()).map_err(e)?.to_string_lossy().to_string();
    }
    let final_dir = dir().join("launcher").join(&ver);
    let previous = dir().join("launcher").join(format!(".previous-{ver}"));
    let _ = fs::remove_dir_all(&previous);
    let had_previous = final_dir.exists();
    if had_previous { fs::rename(&final_dir, &previous).map_err(e)?; }
    if let Err(error) = fs::rename(&staging, &final_dir) {
        if had_previous { let _ = fs::rename(&previous, &final_dir); }
        return Err(e(error));
    }
    write_json(&dir().join("installed.json"), &json!({"version": ver, "exe": exe_rel}));
    let _ = fs::remove_dir_all(&previous);
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
    c.env("HORIZON_LAUNCHER_HOME", runtime_dir());
    c.env("HORIZON_HOME", game_dir());
    let setting_id = load_settings().ms_client_id;
    let client_id = if !setting_id.trim().is_empty() {
        Some(setting_id.trim().to_owned())
    } else {
        std::env::var("NOXEL_HORIZON_MSA_CLIENT_ID").ok()
            .or_else(|| option_env!("NOXEL_HORIZON_MSA_CLIENT_ID").map(str::to_owned))
            .filter(|id| !id.trim().is_empty())
    };
    if let Some(id) = client_id { c.env("HORIZON_MSA_CLIENT_ID", id); }
    else { c.env_remove("HORIZON_MSA_CLIENT_ID"); }
    // Noxel doesn't expose Horizon's telemetry controls; suppress both the live ping and
    // optional telemetry while this client runs from Noxel.
    c.env("HORIZON_TELEMETRY_URL", "off");
    if let Some(p) = exe.parent() { c.current_dir(p); }
    c
}

/// Запуск выбранного профиля командой `horizon-launcher play`; выбранный аккаунт проверяется самим форком.
pub fn play() -> R<()> {
    let (_, exe) = installed().ok_or("Horizon ещё не установлен")?;
    let state = status()?;
    let tag = state["selected_build"].as_str().ok_or("Сначала выбери версию Rust-клиента")?;
    known_build(tag)?;
    let installed = state["installed_builds"].as_array().is_some_and(|versions| versions.iter().any(|v| v.as_str() == Some(tag)));
    if !installed { return Err(format!("Версия {tag} ещё не установлена. Нажми «Установить выбранную версию».")); }
    let mut child = command(&exe).arg("play").arg("--version").arg(tag).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|x| format!("не удалось запустить Horizon: {x}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel();
    let out_tx = tx.clone();
    if let Some(mut out) = stdout {
        thread::spawn(move || { let mut s = String::new(); let _ = out.read_to_string(&mut s); let _ = out_tx.send((0u8, s)); });
    } else { let _ = tx.send((0, String::new())); }
    if let Some(mut err) = stderr {
        thread::spawn(move || { let mut s = String::new(); let _ = err.read_to_string(&mut s); let _ = tx.send((1u8, s)); });
    } else { let _ = tx.send((1, String::new())); }

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.try_wait().map_err(e)? {
            if status.success() { return Ok(()); }
            let mut messages = [String::new(), String::new()];
            for _ in 0..2 {
                if let Ok((i, s)) = rx.recv_timeout(Duration::from_millis(250)) { messages[i as usize] = s; }
            }
            let text = if messages[1].trim().is_empty() { &messages[0] } else { &messages[1] };
            let tail = text.lines().filter(|line| !line.trim().is_empty()).rev().take(5).collect::<Vec<_>>();
            let detail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(if detail.is_empty() { format!("Horizon завершился с кодом {}. Проверь вход через Microsoft и установленную игру.", status.code().unwrap_or(-1)) } else { detail });
        }
        if Instant::now() >= deadline {
            thread::spawn(move || { let _ = child.wait(); });
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Обновляет официальный индекс через CLI Horizon (его SHA/подписи/кэш), затем показывает только
/// автономные .hmod-пакеты для 26.3. Нативные .hnmod исполняют Rust-код и требуют отдельной сборки.
pub fn mods_search(query: &str) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент Horizon")?;
    command_output(&exe, &["--json", "mods", "search", "--kind", "mod", "--source", "horizon", "--instance", "horizon"])?;
    let index_path = game_dir().join("mod-browser/horizon-index.json");
    let index: Value = read_json(&index_path).ok_or("Horizon не сохранил каталог модов")?;
    if index["schema"].as_u64() != Some(1) {
        return Err("Версия каталога модов Horizon не поддерживается установленным клиентом".into());
    }
    let needle = query.trim().to_lowercase();
    let mut items = Vec::new();
    if let Some(projects) = index["projects"].as_array() {
        for project in projects {
            if project["kind"].as_str() != Some("mod") { continue; }
            let Some(version) = project["versions"].as_array().and_then(|versions| versions.iter().find(|v| v["game"].as_str() == Some("26.3"))) else { continue };
            if version["api"].as_str() != Some("0.2") { continue; }
            // Content packages that declare runtime capabilities are executable extensions,
            // and asset-only projects need a separate native mod. Keep the first-party
            // catalog limited to standalone data packs with no host-code requirements.
            if version["capabilities"].as_array().is_some_and(|caps| caps.iter().any(|c| c.as_str() != Some("registries"))) { continue; }
            if version["requires"].as_array().is_some_and(|deps| !deps.is_empty()) { continue; }
            if project["name"].as_str().unwrap_or("").ends_with("(assets)") { continue; }
            let id = project["id"].as_str().unwrap_or("");
            let title = project["name"].as_str().unwrap_or(id);
            let summary = project["summary"].as_str().unwrap_or("");
            let authors = project["authors"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            let tags = project["tags"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            let categories = project["categories"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            let searchable = format!("{id} {title} {summary} {authors} {tags} {categories}").to_lowercase();
            if !needle.is_empty() && !searchable.contains(&needle) { continue; }
            let file = &version["file"];
            let url = file["url"].as_str().unwrap_or("");
            let filename = file["name"].as_str().unwrap_or("");
            let checksum = file["sha256"].as_str().unwrap_or("");
            // The CLI performs the complete registry/file/signature checks on install. Do not
            // show malformed entries as actionable results in Noxel.
            if !url.starts_with("https://github.com/horizon-rust/horizon-mods/releases/download/")
                || !filename.ends_with(".hmod") || checksum.len() != 64
                || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) { continue; }
            items.push(json!({
                "id": id, "title": title, "summary": summary, "author": authors,
                "downloads": project["downloads"], "page": format!("https://playhorizon.dev/mods/{id}"),
                "icon": project["icon"], "license": project["license"], "version": version["version"],
                "date": version["published"], "signed": file["signature"].as_str().is_some_and(|s| !s.is_empty()),
                "capabilities": version["capabilities"], "side": version["side"], "file": filename
            }));
        }
    }
    items.sort_by(|a, b| b["downloads"].as_u64().unwrap_or(0).cmp(&a["downloads"].as_u64().unwrap_or(0)).then_with(|| a["title"].as_str().cmp(&b["title"].as_str())));
    Ok(json!({"items": items, "count": items.len(), "index_generated": index["generated"]}))
}

pub fn mods_list() -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент Horizon")?;
    let mut list: Value = serde_json::from_str(&command_output(&exe, &["--json", "mods", "list", "--instance", "horizon"])?)
        .map_err(|_| "Horizon вернул неверный список установленных модов")?;
    let settings = fs::read_to_string(game_dir().join("horizon-mods.txt")).unwrap_or_default();
    if let Some(entries) = list.as_array_mut() {
        entries.retain_mut(|entry| {
            if !entry["file"].as_str().unwrap_or("").starts_with("horizon-mods/") { return false; }
            let id = entry["project"].as_str().unwrap_or("");
            let key = format!("mod.{id}:");
            entry["enabled"] = settings.lines().find_map(|line| line.trim().strip_prefix(&key)).map(|v| v.trim() == "true").unwrap_or(false).into();
            true
        });
    }
    Ok(list)
}

fn known_content_mod(project: &str) -> R<()> {
    let index: Value = read_json(&game_dir().join("mod-browser/horizon-index.json")).ok_or("Сначала обнови каталог Horizon")?;
    let found = index["projects"].as_array().is_some_and(|projects| projects.iter().any(|p| {
            p["id"].as_str() == Some(project) && p["kind"].as_str() == Some("mod") && !p["name"].as_str().unwrap_or("").ends_with("(assets)") && p["versions"].as_array().is_some_and(|versions| versions.iter().any(|v| {
            v["game"].as_str() == Some("26.3") && v["api"].as_str() == Some("0.2")
                && v["requires"].as_array().map_or(true, |deps| deps.is_empty())
                && v["capabilities"].as_array().map_or(true, |caps| caps.iter().all(|c| c.as_str() == Some("registries")))
        }))
    }));
    if found { Ok(()) } else { Err("В каталоге нет совместимого контент-мода Horizon для Minecraft 26.3".into()) }
}

pub fn mods_install(project: &str) -> R<Value> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент Horizon")?;
    // Refresh the signed registry before validating the ID and installing it.
    mods_search("")?;
    known_content_mod(project)?;
    let output = command_output_owned(&exe, &[
        "--json".into(), "mods".into(), "install".into(), project.into(),
        "--source".into(), "horizon".into(), "--instance".into(), "horizon".into()
    ])?;
    serde_json::from_str(&output).map_err(|_| "Horizon установил мод, но вернул неверный ответ".into())
}

pub fn mods_toggle(project: &str, enabled: bool) -> R<()> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент Horizon")?;
    let listed = mods_list()?;
    if !listed.as_array().is_some_and(|entries| entries.iter().any(|e| e["project"].as_str() == Some(project))) {
        return Err("Этот Horizon-мод не установлен".into());
    }
    let action = if enabled { "enable" } else { "disable" };
    command_output_owned(&exe, &["mods".into(), action.into(), project.into(), "--instance".into(), "horizon".into()])?;
    Ok(())
}

pub fn mods_remove(project: &str) -> R<()> {
    let (_, exe) = installed().ok_or("Сначала установи Rust-клиент Horizon")?;
    let listed = mods_list()?;
    if !listed.as_array().is_some_and(|entries| entries.iter().any(|e| e["project"].as_str() == Some(project))) {
        return Err("Этот Horizon-мод не установлен".into());
    }
    command_output_owned(&exe, &["mods".into(), "remove".into(), project.into(), "--instance".into(), "horizon".into()])?;
    Ok(())
}
