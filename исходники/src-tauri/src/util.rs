use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};

pub type R<T> = Result<T, String>;

/// Платформенные различия: Linux и Windows.
pub const OS_NAME: &str = if cfg!(windows) { "windows" } else { "linux" };
pub const CP_SEP: &str = if cfg!(windows) { ";" } else { ":" };
pub const JAVA_EXE: &str = if cfg!(windows) { "bin/java.exe" } else { "bin/java" };
/// Ключ сборки Java в каталоге рантаймов Mojang.
pub const JAVA_KEY: &str = if cfg!(windows) { "windows-x64" } else { "linux" };
pub const PLATFORM: &str = if cfg!(windows) { "windows-x86_64" } else { "linux-x86_64" };

/// Запуск процесса без мелькающего консольного окна на Windows.
pub fn cmd<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

pub fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

/// Папка данных: `данные/` рядом с `готовый продукт/`, иначе ~/.local/share/noxel.
pub fn data_dir() -> PathBuf {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| {
        if let Ok(p) = std::env::var("NOXEL_HOME") {
            return PathBuf::from(p);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                if dir.file_name().map(|n| n == "готовый продукт").unwrap_or(false) {
                    if let Some(root) = dir.parent() {
                        return root.join("данные");
                    }
                }
            }
        }
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // запуск из target/ (cargo run): данные лежат в папке проекта; у установленных копий этого пути нет
        let dev_run = std::env::current_exe().map(|e| e.components().any(|c| c.as_os_str() == "target")).unwrap_or(false);
        if dev_run && dev.join("исходники").is_dir() {
            return dev.join("данные");
        }
        if cfg!(windows) {
            let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
            return PathBuf::from(base).join("Noxel");
        }
        let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", std::env::var("HOME").unwrap_or_else(|_| ".".into())));
        PathBuf::from(base).join("noxel")
    })
    .clone()
}

pub fn read_json<T: DeserializeOwned>(p: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}

pub fn write_json<T: Serialize>(p: &Path, v: &T) {
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_vec_pretty(v) {
        let _ = fs::write(p, s);
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── HTTP ──

pub fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent("Noxel/0.1.0 (minecraft launcher)")
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(40))
            .build()
    })
}

pub fn get_json(url: &str) -> R<Value> {
    agent().get(url).call().map_err(e)?.into_json().map_err(e)
}

/// Клиент без пула соединений: запасной вариант, когда сервер закрыл keep-alive.
fn fresh_agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent("Noxel/0.1.0 (minecraft launcher)")
            .max_idle_connections(0)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(40))
            .build()
    })
}

fn try_download(url: &str, dest: &Path, sha1: Option<&str>, fresh: bool) -> R<()> {
    let ag = if fresh { fresh_agent() } else { agent() };
    let resp = ag.get(url).call().map_err(e)?;
    let mut r = resp.into_reader();
    let tmp = PathBuf::from(format!("{}.part", dest.display()));
    let mut f = fs::File::create(&tmp).map_err(e)?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = r.read(&mut buf).map_err(e)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        f.write_all(&buf[..n]).map_err(e)?;
    }
    drop(f);
    if let Some(s) = sha1 {
        let got = format!("{:x}", h.finalize());
        if !s.is_empty() && !got.eq_ignore_ascii_case(s) {
            let _ = fs::remove_file(&tmp);
            return Err("контрольная сумма не совпала".into());
        }
    }
    fs::rename(&tmp, dest).map_err(e)
}

pub fn download(url: &str, dest: &Path, sha1: Option<&str>) -> R<()> {
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(e)?;
    }
    let mut last = String::new();
    for attempt in 0..5u64 {
        match try_download(url, dest, sha1, attempt > 0) {
            Ok(()) => return Ok(()),
            Err(x) => last = x,
        }
        std::thread::sleep(Duration::from_millis(400 * (attempt + 1)));
    }
    Err(format!("{url}: {last}"))
}

pub struct Job {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
}

/// Параллельная загрузка. `cb(готово, всего)` вызывается из рабочих потоков.
pub fn download_all(jobs: Vec<Job>, threads: usize, cb: &(dyn Fn(u64, u64) + Sync)) -> R<()> {
    let total = jobs.len() as u64;
    if total == 0 {
        return Ok(());
    }
    let queue = Mutex::new(VecDeque::from(jobs));
    let done = AtomicU64::new(0);
    let err: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads.min(total as usize).max(1) {
            s.spawn(|| loop {
                if err.lock().unwrap().is_some() {
                    break;
                }
                let job = queue.lock().unwrap().pop_front();
                let Some(j) = job else { break };
                if j.url.is_empty() {
                    *err.lock().unwrap() = Some(format!("нет ссылки для {}", j.path.display()));
                    break;
                }
                if let Err(x) = download(&j.url, &j.path, j.sha1.as_deref()) {
                    *err.lock().unwrap() = Some(x);
                    break;
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                cb(d, total);
            });
        }
    });
    match err.into_inner().unwrap() {
        Some(x) => Err(x),
        None => Ok(()),
    }
}

// ── настройки ──

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub nickname: String,
    pub ram_mb: u32,
    pub java_path: String,
    pub jvm_args: String,
    pub cf_key: String,
    pub last_profile: String,
    pub width: u32,
    pub height: u32,
    pub ms_client_id: String,
    pub admin_token: String,
    pub auto_auth: bool,
    pub auto_skin: bool,
    pub game_icon: bool,
    /// Автовход и автоскин теперь выключены по умолчанию; у уже установленных копий сбрасываем один раз.
    pub opt_in_v2: bool,
    /// Своё облако по WebDAV (Яндекс Диск: https://webdav.yandex.ru, логин и пароль приложения). Хранится только на этом компьютере.
    pub dav_url: String,
    pub dav_user: String,
    pub dav_pass: String,
    pub dav_auto: bool,
    /// Ставить мод Noxel (кнопка «Друзья» в игре) в профили Fabric для 26.x
    pub noxel_mod: bool,
    /// Аккаунт Noxel входит в игру через сервер Noxel (authlib-injector): скин виден всем в мирах друзей и на серверах с Noxel.
    pub noxel_ygg: bool,
    /// Дополнительные UDP-порты для друзей (голосовой чат: Simple Voice Chat 24454, Plasmo Voice 60606), через запятую
    pub room_ports: String,
    pub auto_skip: Vec<String>,
    pub on_launch: String,
    pub fullscreen: bool,
    pub show_console: bool,
    pub animations: bool,
    pub threads: u32,
}

impl Default for Settings {
    fn default() -> Self {
        let total = total_ram_mb();
        let ram = if total >= 12288 { 4096 } else if total >= 6144 { 3072 } else { 2048 };
        Settings {
            nickname: "Player".into(),
            ram_mb: ram,
            java_path: String::new(),
            jvm_args: "-XX:+UseG1GC".into(),
            cf_key: String::new(),
            last_profile: String::new(),
            width: 0,
            height: 0,
            ms_client_id: String::new(),
            admin_token: String::new(),
            auto_auth: false,
            auto_skin: false,
            game_icon: true,
            opt_in_v2: false,
            dav_url: String::new(),
            dav_user: String::new(),
            dav_pass: String::new(),
            dav_auto: false,
            noxel_mod: true,
            noxel_ygg: false,
            room_ports: "24454".into(),
            auto_skip: vec![],
            on_launch: "keep".into(),
            fullscreen: false,
            show_console: false,
            animations: true,
            threads: 8,
        }
    }
}

#[cfg(windows)]
pub fn total_ram_mb() -> u32 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            return (m.ullTotalPhys / 1_048_576) as u32;
        }
    }
    8192
}

#[cfg(not(windows))]
pub fn total_ram_mb() -> u32 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse::<u64>().ok())
        })
        .map(|kb| (kb / 1024) as u32)
        .unwrap_or(8192)
}

pub fn load_settings() -> Settings {
    let mut s: Settings = read_json(&data_dir().join("settings.json")).unwrap_or_default();
    if !s.opt_in_v2 {
        s.auto_auth = false;
        s.auto_skin = false;
        s.opt_in_v2 = true;
        save_settings(&s);
    }
    s
}

pub fn save_settings(s: &Settings) {
    write_json(&data_dir().join("settings.json"), s);
}

pub fn open_path(target: &str) {
    let mut c = if cfg!(windows) {
        if target.starts_with("http") {
            let mut c = cmd("rundll32");
            c.arg("url.dll,FileProtocolHandler").arg(target);
            c
        } else {
            let mut c = cmd("explorer");
            c.arg(target);
            c
        }
    } else {
        let mut c = cmd("xdg-open");
        c.arg(target);
        c
    };
    let _ = c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

/// Имя файла без путей: защита от выхода за пределы папки профиля.
pub fn safe_name(n: &str) -> R<&str> {
    if n.is_empty() || n.contains('/') || n.contains('\\') || n == "." || n == ".." || n.contains('\0') {
        return Err("недопустимое имя файла".into());
    }
    Ok(n)
}
