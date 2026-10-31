//! «Локальная сеть» через ретранслятор Noxel: хозяин мира держит управляющее соединение с сервером,
//! гость подключается к локальному порту лаунчера, и байты идут через сервер к хозяину.
//! Хозяин пересылает гостям только на свой локальный порт игры: адрес от сервера не берётся.
use crate::skins::{call, Body};
use crate::util::*;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

/// Порт мира, открытого для сети в игре (из лога), 0 если не открыт.
pub static LAN_PORT: AtomicU16 = AtomicU16::new(0);

struct Host {
    stop: Arc<AtomicBool>,
    ctl: Arc<Mutex<Option<TcpStream>>>,
    port: u16,
}
static HOST: Mutex<Option<Host>> = Mutex::new(None);
static GUEST: Mutex<Option<(Arc<AtomicBool>, String, u16)>> = Mutex::new(None);

/// Ищет в строке лога сообщение «Local game hosted on port N» (и русский вариант) и запоминает порт.
pub fn scan_line(line: &str) -> Option<u16> {
    let low = line.to_lowercase();
    if !(low.contains("[chat]") && (low.contains("local game hosted") || low.contains("локальная игра"))) {
        return None;
    }
    let digits: String = line.chars().rev().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
    let port: u16 = digits.parse().ok()?;
    LAN_PORT.store(port, Ordering::Relaxed);
    Some(port)
}

fn ticket(token: &str) -> R<(String, String)> {
    let r = call("POST", "/relay/ticket", Some(token), Body::None)?;
    let t = r["ticket"].as_str().ok_or("сервер не выдал билет")?.to_string();
    let addr = format!("{}:{}", r["host"].as_str().unwrap_or("mr1s.site"), r["port"].as_u64().unwrap_or(8731));
    Ok((t, addr))
}

fn connect(addr: &str) -> R<TcpStream> {
    use std::net::ToSocketAddrs;
    let a = addr.to_socket_addrs().map_err(e)?.next().ok_or("адрес ретранслятора не найден")?;
    let s = TcpStream::connect_timeout(&a, Duration::from_secs(8)).map_err(|x| format!("нет связи с сервером Noxel: {x}"))?;
    let _ = s.set_nodelay(true);
    Ok(s)
}

fn read_line(s: &mut TcpStream) -> Option<String> {
    let mut out = vec![];
    let mut b = [0u8; 1];
    while out.len() < 300 {
        match s.read(&mut b) {
            Ok(1) if b[0] == b'\n' => return Some(String::from_utf8_lossy(&out).trim().to_string()),
            Ok(1) => out.push(b[0]),
            _ => return None,
        }
    }
    None
}

/// Пересылает байты в обе стороны, пока одна из сторон не закроется.
fn pipe(a: TcpStream, b: TcpStream) {
    let (mut a2, mut b2) = match (a.try_clone(), b.try_clone()) {
        (Ok(x), Ok(y)) => (x, y),
        _ => return,
    };
    let t = thread::spawn(move || {
        let _ = std::io::copy(&mut b2, &mut a2);
        let _ = a2.shutdown(Shutdown::Both);
        let _ = b2.shutdown(Shutdown::Both);
    });
    let (mut a, mut b) = (a, b);
    let _ = std::io::copy(&mut a, &mut b);
    let _ = a.shutdown(Shutdown::Both);
    let _ = b.shutdown(Shutdown::Both);
    let _ = t.join();
}

// ── хозяин ──

fn host_session(relay: String, id: String, port: u16) {
    let (Ok(mut r), Ok(l)) = (connect(&relay), TcpStream::connect(("127.0.0.1", port))) else { return };
    let _ = l.set_nodelay(true);
    if r.write_all(format!("DATA {id}\n").as_bytes()).is_err() {
        return;
    }
    pipe(r, l);
}

fn host_run(token: &str, port: u16, stop: &Arc<AtomicBool>, slot: &Arc<Mutex<Option<TcpStream>>>) -> R<()> {
    let (t, relay) = ticket(token)?;
    let mut s = connect(&relay)?;
    s.write_all(format!("HOST {t}\n").as_bytes()).map_err(e)?;
    s.set_read_timeout(Some(Duration::from_secs(10))).map_err(e)?;
    if read_line(&mut s).as_deref() != Some("OK") {
        return Err("сервер не принял открытие мира".into());
    }
    s.set_read_timeout(None).map_err(e)?;
    *slot.lock().unwrap() = s.try_clone().ok();
    let mut pinger = s.try_clone().map_err(e)?;
    let stop2 = stop.clone();
    thread::spawn(move || {
        let mut last = Instant::now();
        while !stop2.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
            if last.elapsed() >= Duration::from_secs(20) {
                last = Instant::now();
                if pinger.write_all(b"PING\n").is_err() {
                    break;
                }
            }
        }
    });
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        if let Some(id) = line.strip_prefix("CONN ") {
            let (relay, id) = (relay.clone(), id.trim().to_string());
            thread::spawn(move || host_session(relay, id, port));
        }
    }
    Ok(())
}

pub fn host_open(token: &str, port: u16) -> R<()> {
    host_close();
    // проверяем сразу, чтобы ошибка показалась пользователю, а не потерялась в потоке
    ticket(token)?;
    let stop = Arc::new(AtomicBool::new(false));
    let slot: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let (token, stop2, slot2) = (token.to_string(), stop.clone(), slot.clone());
    thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            if let Err(x) = host_run(&token, port, &stop2, &slot2) {
                eprintln!("[комната] {x}");
            }
            for _ in 0..6 {
                if stop2.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_millis(500));
            }
        }
    });
    *HOST.lock().unwrap() = Some(Host { stop, ctl: slot, port });
    Ok(())
}

pub fn host_close() {
    if let Some(h) = HOST.lock().unwrap().take() {
        h.stop.store(true, Ordering::Relaxed);
        if let Some(s) = h.ctl.lock().unwrap().take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

pub fn host_port() -> Option<u16> {
    HOST.lock().unwrap().as_ref().map(|h| h.port)
}

// ── гость ──

/// Открывает локальный порт, подключения к которому уходят через сервер к миру друга. Возвращает порт.
pub fn guest_open(token: &str, host_name: &str) -> R<u16> {
    guest_close();
    let (first, relay) = ticket(token)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(e)?;
    let port = listener.local_addr().map_err(e)?.port();
    listener.set_nonblocking(true).map_err(e)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (token, host, stop2) = (token.to_string(), host_name.to_string(), stop.clone());
    thread::spawn(move || {
        let mut tk = (first, Instant::now());
        while !stop2.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((c, _)) => {
                    let _ = c.set_nonblocking(false);
                    let _ = c.set_nodelay(true);
                    if tk.1.elapsed() > Duration::from_secs(480) {
                        if let Ok((t, _)) = ticket(&token) {
                            tk = (t, Instant::now());
                        }
                    }
                    let (relay, t, host) = (relay.clone(), tk.0.clone(), host.clone());
                    thread::spawn(move || {
                        let Ok(mut r) = connect(&relay) else { return };
                        if r.write_all(format!("JOIN {t} {host}\n").as_bytes()).is_err() {
                            return;
                        }
                        let _ = r.set_read_timeout(Some(Duration::from_secs(15)));
                        if read_line(&mut r).as_deref() != Some("OK") {
                            return;
                        }
                        let _ = r.set_read_timeout(None);
                        pipe(c, r);
                    });
                }
                Err(ref x) if x.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(60)),
                Err(_) => break,
            }
        }
    });
    *GUEST.lock().unwrap() = Some((stop, host_name.to_string(), port));
    Ok(port)
}

pub fn guest_close() {
    if let Some((stop, _, _)) = GUEST.lock().unwrap().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

pub fn state() -> Value {
    serde_json::json!({
        "lan_port": LAN_PORT.load(Ordering::Relaxed),
        "hosting": host_port(),
        "guest": GUEST.lock().unwrap().as_ref().map(|(_, n, p)| serde_json::json!({"host": n, "port": p})),
    })
}

/// Открывает запущенный одиночный мир для друзей: агент публикует мир на свободном порту, лаунчер соединяет порт с сервером.
pub fn open_world(token: &str, profile_id: &str) -> R<Value> {
    use crate::autoauth;
    let dir = crate::mc::inst_dir(profile_id);
    let port = TcpListener::bind("127.0.0.1:0").map_err(e)?.local_addr().map_err(e)?.port();
    let cid = autoauth::control(&dir, &format!("!publish {port}"));
    match autoauth::wait_ack(&dir, &cid, 6).as_deref() {
        Some("sent") => {
            host_open(token, port)?;
            Ok(serde_json::json!({"port": port, "how": "auto"}))
        }
        Some("error:no-world") => Err("Зайди в одиночный мир (не на сервер) и повтори".into()),
        other => {
            // версии без читаемых имён классов: пользователь открывает мир сам (Esc → «Открыть для сети»), порт берём из лога
            let lan = LAN_PORT.load(Ordering::Relaxed);
            if lan > 0 {
                host_open(token, lan)?;
                Ok(serde_json::json!({"port": lan, "how": "lan"}))
            } else {
                Err(format!("Эта версия игры не умеет открывать мир сама ({}). В игре нажми Esc → «Открыть для сети», потом повтори", other.unwrap_or("нет ответа")))
            }
        }
    }
}

pub fn close_world(profile_id: Option<&str>) {
    if let (Some(pid), Some(_)) = (profile_id, host_port()) {
        crate::autoauth::control(&crate::mc::inst_dir(pid), "!unpublish");
    }
    host_close();
    guest_close();
}
