#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod autoauth;
mod ping;
mod mc;
mod mods;
mod packs;
mod remote;
mod skins;
mod stats;
mod worlds;
#[cfg(test)]
mod selftest;
mod util;

use serde_json::{json, Value};
use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use util::*;

type Game = mc::Game;

/// Колбэк прогресса, который шлёт события в окно (не чаще ~12 раз в секунду).
fn progress_cb(app: &AppHandle, id: String) -> impl Fn(&str, f64, &str) + Sync + '_ {
    let start = Instant::now();
    let last = AtomicU64::new(0);
    move |stage, frac, detail| {
        let t = start.elapsed().as_millis() as u64;
        let prev = last.load(Ordering::Relaxed);
        if frac < 1.0 && t.saturating_sub(prev) < 80 {
            return;
        }
        last.store(t, Ordering::Relaxed);
        let _ = app.emit("task", json!({"id": id, "stage": stage, "frac": frac, "detail": detail}));
    }
}

// ── версии и профили ──

#[tauri::command(async)]
fn versions() -> R<Value> {
    let m = mc::manifest()?;
    let list: Vec<Value> = m["versions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|v| json!({"id": v["id"], "type": v["type"], "time": v["releaseTime"]}))
        .collect();
    Ok(json!({"latest": m["latest"], "versions": list}))
}

#[tauri::command(async)]
fn loader_versions(mc: String, loader: String) -> Vec<String> {
    mc::loader_versions(&mc, &loader)
}

#[tauri::command]
fn profiles() -> Vec<Value> {
    mc::load_profiles().iter().map(mc::profile_view).collect()
}

#[tauri::command(async)]
fn create_profile(app: AppHandle, mc: String, loader: String, loader_ver: Option<String>, name: String, preset: Option<String>) -> R<Value> {
    let preset = preset.filter(|s| !s.is_empty());
    let p = mc::create_profile(&mc, &loader, loader_ver.filter(|s| !s.is_empty()), &name, preset.clone())?;
    let (mut installed, mut skipped) = (vec![], vec![]);
    if let Some(pr) = preset {
        let cb = progress_cb(&app, format!("mod:preset-{}", p.id));
        match mods::apply_preset(&p.id, &pr, &cb) {
            Ok((i, s)) => (installed, skipped) = (i, s),
            Err(x) => skipped.push(x),
        }
    }
    Ok(json!({"profile": mc::profile_view(&mc::find_profile(&p.id)?), "installed": installed, "skipped": skipped}))
}

#[tauri::command]
fn presets() -> Value {
    mods::presets()
}

#[tauri::command(async)]
fn delete_profile(id: String, remove_files: bool) -> R<()> {
    mc::delete_profile(&id, remove_files)
}

// ── запуск ──

fn start(app: AppHandle, id: String, play: bool) -> R<()> {
    let game = app.state::<Game>();
    if game.busy.swap(true, Ordering::SeqCst) {
        return Err("Уже идёт установка или игра запущена".into());
    }
    std::thread::spawn(move || {
        let game = app.state::<Game>();
        let ev = |state: &str, extra: Value| {
            let mut v = json!({"id": id, "state": state});
            if let (Some(o), Some(x)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(x.clone());
            }
            let _ = app.emit("game", v);
        };
        let res = (|| -> R<()> {
            let p = mc::find_profile(&id)?;
            let mut st = load_settings();
            st.last_profile = id.clone();
            save_settings(&st);
            ev("installing", json!({}));
            let cb = progress_cb(&app, id.clone());
            cb("Аккаунт", -1.0, "проверяю вход");
            let acct = accounts::active_fresh()?;
            let pr = mc::prepare(&p, &st, &acct, &cb)?;
            if !play {
                return Ok(());
            }
            let sid = stats::begin(&acct.id, &p.id, &p.mc, &p.loader);
            let auth = {
                let app3 = app.clone();
                autoauth::Auth::new(&acct.id, &pr.gamedir, Box::new(move |kind, host| {
                    let _ = app3.emit("autoauth", json!({"kind": kind, "host": host}));
                }))
                .ok()
                .map(|a| Arc::new(std::sync::Mutex::new(a)))
            };
            let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
            if let Some(a) = auth.clone() {
                let alive = running.clone();
                std::thread::spawn(move || {
                    while alive.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(500));
                        a.lock().unwrap().tick();
                    }
                });
            }
            let (acc_id, sid2) = (acct.id.clone(), sid.clone());
            let app2 = app.clone();
            // игра упала из-за нашего агента: запоминаем, чтобы отключить его и не оставить игрока с вечным крашем
            let agent_fault = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let fault2 = agent_fault.clone();
            let emit_log: Arc<dyn Fn(Vec<String>) + Send + Sync> = Arc::new(move |lines| {
                let g = app2.state::<Game>();
                {
                    let mut l = g.logs.lock().unwrap();
                    l.extend(lines.iter().cloned());
                    let n = l.len();
                    if n > 4000 {
                        l.drain(..n - 4000);
                    }
                }
                for l in &lines {
                    if l.contains("noxel/Hook") || l.contains("noxel.Hook") || l.contains("noxel/Ticker") || l.contains("noxel-agent") || l.contains("noxel/Patcher") {
                        fault2.store(true, Ordering::Relaxed);
                    }
                    if let Some(h) = stats::server_from_log(l) {
                        stats::update(&acc_id, &sid2, |s| {
                            if !s.servers.contains(&h) {
                                s.servers.push(h.clone());
                            }
                        });
                    }
                }
                let _ = app2.emit("log", lines);
            });
            let acc_id = acct.id.clone();
            let started = Instant::now();
            let auth_line = auth.clone();
            let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |l| {
                if let Some(a) = &auth_line {
                    a.lock().unwrap().on_line(l);
                }
            });
            let child = mc::spawn_game(&game, &pr, emit_log, on_line)?;
            *game.running.lock().unwrap() = Some(id.clone());
            *game.child.lock().unwrap() = Some(child);
            let _ = app.emit("profiles-changed", ());
            let now = now();
            // last_played обновляем через профили
            {
                let mut all = mc::load_profiles();
                if let Some(x) = all.iter_mut().find(|x| x.id == id) {
                    x.last_played = Some(now);
                }
                write_json(&data_dir().join("profiles.json"), &all);
            }
            ev("running", json!({}));
            if st.on_launch == "minimize" {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.minimize();
                }
            }
            let mut tick = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(400));
                tick += 1;
                if tick % 150 == 0 {
                    stats::update(&acc_id, &sid, |s| s.secs = started.elapsed().as_secs());
                }
                let mut g = game.child.lock().unwrap();
                match g.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(Some(status))) => {
                        let code = status.code().unwrap_or(-1);
                        *g = None;
                        drop(g);
                        if code != 0 && agent_fault.load(Ordering::Relaxed) {
                            // агент лаунчера мешает этой игре: отключаем его, следующий запуск пройдёт без него
                            let mut s = load_settings();
                            s.auto_auth = false;
                            s.auto_skin = false;
                            s.game_icon = false;
                            save_settings(&s);
                            ev("agent-off", json!({}));
                        }
                        ev("exited", json!({"code": code}));
                        break;
                    }
                    Some(Err(_)) | None => {
                        drop(g);
                        ev("exited", json!({"code": 0}));
                        break;
                    }
                    _ => {}
                }
            }
            stats::update(&acc_id, &sid, |s| s.secs = started.elapsed().as_secs());
            running.store(false, Ordering::Relaxed);
            let acc_sync = acc_id.clone();
            std::thread::spawn(move || accounts::sync_up(&acc_sync));
            Ok(())
        })();
        match res {
            Ok(()) => {
                if !play {
                    ev("installed", json!({}));
                }
            }
            Err(x) => ev("error", json!({"message": x})),
        }
        *game.running.lock().unwrap() = None;
        game.busy.store(false, Ordering::SeqCst);
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        let _ = app.emit("profiles-changed", ());
    });
    Ok(())
}

#[tauri::command]
fn launch(app: AppHandle, id: String) -> R<()> {
    start(app, id, true)
}

#[tauri::command]
fn launch_to(app: AppHandle, id: String, host: String) -> R<()> {
    *mc::JOIN.lock().unwrap() = Some(host);
    start(app, id, true)
}

#[tauri::command]
fn install_profile(app: AppHandle, id: String) -> R<()> {
    start(app, id, false)
}

#[tauri::command]
fn stop_game(game: State<'_, Game>) {
    if let Some(c) = game.child.lock().unwrap().as_mut() {
        let _ = c.kill();
    }
}

#[tauri::command]
fn game_status(game: State<'_, Game>) -> Value {
    json!({"busy": game.busy.load(Ordering::SeqCst), "running": *game.running.lock().unwrap()})
}

#[tauri::command]
fn get_logs(game: State<'_, Game>) -> Vec<String> {
    game.logs.lock().unwrap().clone()
}

// ── настройки ──

#[tauri::command]
fn settings_get() -> Settings {
    load_settings()
}

#[tauri::command]
fn settings_set(mut s: Settings) -> Settings {
    s.threads = s.threads.clamp(2, 24);
    if s.on_launch != "minimize" {
        s.on_launch = "keep".into();
    }
    s.ram_mb = s.ram_mb.clamp(512, total_ram_mb().max(1024));
    save_settings(&s);
    s
}

#[tauri::command]
fn sys_info() -> Value {
    json!({"total_ram_mb": total_ram_mb(), "data_dir": data_dir().to_string_lossy(), "zlib_ng": mc::system_zlib_is_ng(), "start_page": std::env::var("NOXEL_PAGE").ok(), "version": remote::VERSION})
}

#[tauri::command(async)]
fn clear_cache() -> R<u64> {
    let dir = data_dir().join("cache");
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(&dir) {
        for f in rd.flatten() {
            n += f.metadata().map(|m| m.len()).unwrap_or(0);
            let _ = fs::remove_file(f.path());
        }
    }
    Ok(n)
}

// ── аккаунты ──

#[tauri::command(async)]
fn accounts_list() -> Value {
    accounts::list()
}

#[tauri::command(async)]
fn account_add_local(name: String) -> R<Value> {
    accounts::add_local(&name)?;
    Ok(accounts::list())
}

#[tauri::command(async)]
fn account_remove(id: String) -> R<Value> {
    accounts::remove(&id)?;
    Ok(accounts::list())
}

#[tauri::command(async)]
fn account_select(id: String) -> R<Value> {
    accounts::select(&id)?;
    Ok(accounts::list())
}

#[tauri::command(async)]
fn noxel_auth(name: String, password: String, register: bool) -> R<Value> {
    accounts::noxel_auth(&name, &password, register)?;
    Ok(accounts::list())
}

#[tauri::command(async)]
fn skin_data(hash: String) -> R<String> {
    skins::data_url(&hash)
}

#[tauri::command(async)]
fn account_details(id: String, refresh: bool) -> R<Value> {
    accounts::details(&id, refresh)
}

#[tauri::command(async)]
fn skin_apply(id: String, path: String, model: Option<String>) -> R<()> {
    accounts::skin_apply(&id, &path, model)
}

#[tauri::command(async)]
fn skin_use(id: String, entry: u64) -> R<()> {
    accounts::skin_use(&id, entry)
}

#[tauri::command(async)]
fn skin_remove(id: String, entry: u64) -> R<()> {
    accounts::skin_remove(&id, entry)
}

#[tauri::command(async)]
fn cape_apply(id: String, path: String) -> R<()> {
    accounts::cape_apply(&id, &path)
}

#[tauri::command(async)]
fn cape_apply_data(id: String, b64: String) -> R<()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| "повреждённая картинка")?;
    accounts::cape_apply_bytes(&id, &bytes)
}

#[tauri::command(async)]
fn ms_capes(id: String) -> R<Value> {
    accounts::ms_capes(&id)
}

#[tauri::command(async)]
fn ms_cape_set(id: String, cape: Option<String>) -> R<()> {
    accounts::ms_cape_set(&id, cape)
}

#[tauri::command(async)]
fn cape_clear(id: String) -> R<()> {
    accounts::cape_clear(&id)
}

#[tauri::command(async)]
fn duplicate_profile(id: String) -> R<Value> {
    Ok(mc::profile_view(&mc::duplicate_profile(&id)?))
}

#[tauri::command(async)]
fn server_status(host: String) -> Value {
    ping::status(&host)
}

#[tauri::command]
fn server_pw_get(id: String) -> R<String> {
    accounts::server_pw_get(&id)
}

#[tauri::command]
fn server_pw_set(id: String, pw: String) -> R<()> {
    accounts::server_pw_set(&id, &pw)
}

#[tauri::command(async)]
fn server_password(account_id: String, host: String) -> R<String> {
    accounts::server_password_for(&account_id, &host)
}

#[tauri::command]
fn autoauth_toggle(host: String) -> Vec<String> {
    let mut s = load_settings();
    if let Some(i) = s.auto_skip.iter().position(|x| *x == host) {
        s.auto_skip.remove(i);
    } else {
        s.auto_skip.push(host);
    }
    save_settings(&s);
    s.auto_skip
}

#[tauri::command(async)]
fn news_list() -> R<Value> {
    remote::news()
}

#[tauri::command(async)]
fn news_save(list: Value) -> R<Value> {
    remote::news_save(list)
}

#[tauri::command(async)]
fn servers_list() -> R<Value> {
    remote::servers()
}

#[tauri::command(async)]
fn servers_save(list: Value) -> R<Value> {
    remote::servers_save(list)
}

#[tauri::command(async)]
fn servers_install(app: AppHandle, profile_id: String, server_id: String) -> R<Value> {
    let all = remote::servers()?;
    let srv = all["servers"].as_array().and_then(|a| a.iter().find(|s| s["id"] == server_id)).ok_or("сервер не найден в списке")?;
    let list: Vec<(String, String)> = srv["mods"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| Some((m["provider"].as_str()?.to_string(), m["id"].as_str()?.to_string()))).collect();
    let cb = progress_cb(&app, format!("mod:server-{profile_id}"));
    let (ok, skipped) = mods::install_list(&profile_id, &list, &cb)?;
    Ok(json!({"installed": ok, "skipped": skipped}))
}

#[tauri::command]
fn worlds_list(profile_id: String) -> R<Value> {
    worlds::list(&profile_id)
}

#[tauri::command(async)]
fn world_backup(profile_id: String, world: String) -> R<Value> {
    worlds::backup(&profile_id, &world)
}

#[tauri::command(async)]
fn backup_restore(profile_id: String, file: String) -> R<Value> {
    worlds::restore(&profile_id, &file)
}

#[tauri::command]
fn backup_delete(profile_id: String, file: String) -> R<()> {
    worlds::delete(&profile_id, &file)
}

#[tauri::command(async)]
fn skin_from_nick(id: String, nick: String) -> R<String> {
    accounts::skin_from_nick(&id, &nick)
}

#[tauri::command(async)]
fn packs_search(query: String, sort: String, offset: u32, mc: Option<String>) -> R<Value> {
    packs::search(&query, &sort, offset, mc.as_deref().unwrap_or(""))
}

#[tauri::command(async)]
fn packs_install(app: AppHandle, provider: String, project_id: String) -> R<Value> {
    let cb = progress_cb(&app, format!("pack:{project_id}"));
    let r = packs::install(&provider, &project_id, &cb)?;
    let _ = app.emit("profiles-changed", ());
    Ok(r)
}

#[tauri::command(async)]
fn launcher_check() -> R<Value> {
    remote::check()
}

#[tauri::command(async)]
fn launcher_update() -> R<String> {
    remote::update()
}

#[tauri::command]
fn launcher_restart(app: AppHandle) -> R<()> {
    remote::restart()?;
    app.exit(0);
    Ok(())
}

#[tauri::command(async)]
fn ms_login_start() -> R<Value> {
    accounts::ms_start()
}

#[tauri::command(async)]
fn ms_login_wait(device_code: String, interval: u64, expires_in: u64) -> R<Value> {
    accounts::ms_wait(&device_code, interval, expires_in)?;
    Ok(accounts::list())
}

#[tauri::command]
fn ms_login_cancel() {
    accounts::ms_cancel()
}

// ── моды ──

#[tauri::command(async)]
fn mods_list(profile_id: String, kind: String) -> R<Value> {
    mods::list(&profile_id, &kind)
}

#[tauri::command(async)]
fn mods_toggle(profile_id: String, kind: String, file: String) -> R<()> {
    mods::toggle(&profile_id, &kind, &file)
}

#[tauri::command(async)]
fn mods_remove(profile_id: String, kind: String, file: String) -> R<()> {
    mods::remove(&profile_id, &kind, &file)
}

#[tauri::command(async)]
fn mods_add(profile_id: String, kind: String, paths: Vec<String>) -> R<Vec<String>> {
    mods::add_local(&profile_id, &kind, &paths)
}

#[tauri::command(async)]
fn mods_search(provider: String, kind: String, query: String, profile_id: String, sort: String, offset: u32, client_only: bool) -> R<Value> {
    let p = mc::find_profile(&profile_id)?;
    if provider == "all" {
        return mods::search_all(&kind, &query, &p, &sort, offset, client_only);
    }
    mods::search(&provider, &kind, &query, &p, &sort, offset, client_only)
}

#[derive(serde::Deserialize)]
struct SourceRef {
    provider: String,
    id: String,
}

#[tauri::command(async)]
fn mod_details_all(kind: String, sources: Vec<SourceRef>, profile_id: String) -> R<Value> {
    let s: Vec<(String, String)> = sources.into_iter().map(|x| (x.provider, x.id)).collect();
    mods::details_all(&kind, &s, &profile_id)
}

#[tauri::command(async)]
fn mod_details(provider: String, kind: String, project_id: String, profile_id: String) -> R<Value> {
    mods::details(&provider, &project_id, &profile_id, &kind)
}

#[tauri::command(async)]
fn mods_check_updates(profile_id: String) -> R<Value> {
    mods::check_updates(&profile_id)
}

#[tauri::command(async)]
fn mods_update(app: AppHandle, profile_id: String, project_ids: Vec<String>) -> R<Vec<String>> {
    let cb = progress_cb(&app, format!("mod:update-{profile_id}"));
    mods::update_mods(&profile_id, &project_ids, &cb)
}

#[tauri::command(async)]
fn mods_install(app: AppHandle, provider: String, kind: String, project_id: String, profile_id: String, version: Option<String>) -> R<Vec<String>> {
    let cb = progress_cb(&app, format!("mod:{project_id}"));
    mods::install(&provider, &kind, &project_id, &profile_id, version, &cb)
}

// ── прочее ──

#[tauri::command]
fn open_folder(profile_id: Option<String>, sub: Option<String>) -> R<()> {
    let mut p = match profile_id {
        Some(id) => {
            safe_name(&id)?;
            mc::inst_dir(&id)
        }
        None => data_dir(),
    };
    if let Some(s) = sub {
        p = p.join(mods::folder(&s).unwrap_or("mods"));
    }
    fs::create_dir_all(&p).map_err(e)?;
    open_path(&p.to_string_lossy());
    Ok(())
}

#[tauri::command]
fn open_url(url: String) -> R<()> {
    if url.starts_with("https://") || url.starts_with("http://") {
        open_path(&url);
        Ok(())
    } else {
        Err("ссылка не поддерживается".into())
    }
}

/// `Noxel --prefetch [версия|latest]`: скачать Java и игру без окна. Печатает строки PROGRESS/DONE/ERROR для установщика.
fn prefetch_cli(version: Option<String>, java_only: bool) -> i32 {
    let last = AtomicU64::new(0);
    let start = Instant::now();
    let say = |stage: &str, frac: f64, detail: &str| {
        let t = start.elapsed().as_millis() as u64;
        if frac < 1.0 && t.saturating_sub(last.load(Ordering::Relaxed)) < 120 {
            return;
        }
        last.store(t, Ordering::Relaxed);
        println!("PROGRESS\t{stage}\t{frac:.3}\t{detail}");
    };
    let threads = load_settings().threads.clamp(2, 24) as usize;
    match mc::prefetch(version.as_deref().unwrap_or("latest"), threads, java_only, &say) {
        Ok(id) => {
            println!("DONE\t{id}");
            0
        }
        Err(x) => {
            println!("ERROR\t{}", x.replace('\n', " "));
            1
        }
    }
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
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "--prefetch").unwrap_or(false) {
        let _ = fs::create_dir_all(data_dir());
        std::process::exit(prefetch_cli(args.get(2).filter(|a| !a.starts_with("--")).cloned(), args.iter().any(|a| a == "--only-java")));
    }
    #[cfg(windows)]
    ensure_webview2();
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(exe.with_extension("old"));
    }
    let _ = fs::create_dir_all(data_dir());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Game::default())
        .invoke_handler(tauri::generate_handler![
            versions,
            loader_versions,
            profiles,
            create_profile,
            presets,
            delete_profile,
            clear_cache,
            accounts_list,
            servers_list,
            packs_search,
            worlds_list,
            world_backup,
            backup_restore,
            backup_delete,
            skin_from_nick,
            packs_install,
            news_list,
            news_save,
            server_status,
            launch_to,
            server_password,
            server_pw_get,
            server_pw_set,
            autoauth_toggle,
            servers_save,
            servers_install,
            launcher_check,
            launcher_update,
            launcher_restart,
            noxel_auth,
            skin_data,
            account_details,
            skin_apply,
            skin_use,
            skin_remove,
            cape_apply,
            cape_apply_data,
            ms_capes,
            ms_cape_set,
            cape_clear,
            duplicate_profile,
            account_add_local,
            account_remove,
            account_select,
            ms_login_start,
            ms_login_wait,
            ms_login_cancel,
            mods_add,
            launch,
            install_profile,
            stop_game,
            game_status,
            get_logs,
            settings_get,
            settings_set,
            sys_info,
            mods_list,
            mods_toggle,
            mods_remove,
            mods_search,
            mod_details,
            mod_details_all,
            mods_check_updates,
            mods_update,
            mods_install,
            open_folder,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Noxel");
}
