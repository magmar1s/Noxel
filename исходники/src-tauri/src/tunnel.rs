//! «Локальная сеть» через ретранслятор Noxel: хозяин мира держит управляющее соединение с сервером,
//! гость подключается к локальному порту лаунчера, и байты идут через сервер к хозяину.
//! Хозяин пересылает гостям только на свой локальный порт игры: адрес от сервера не берётся.
use crate::skins::{call, Body};
use crate::util::*;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{ToSocketAddrs, UdpSocket};
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

/// Дополнительные UDP-порты друзей из настроек (голосовой чат и т. п.).
pub fn room_ports() -> Vec<u16> {
    load_settings().room_ports.split(|c: char| c == ',' || c == ' ' || c == ';').filter_map(|x| x.trim().parse::<u16>().ok()).filter(|p| *p >= 1024).take(4).collect()
}

fn unhex4(s: &str) -> Option<[u8; 4]> {
    if s.len() != 8 {
        return None;
    }
    let mut o = [0u8; 4];
    for i in 0..4 {
        o[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(o)
}

/// Хозяин: пересылает UDP между сервером и своим локальным портом (например, голосовой чат), пока поток не закроют.
fn udp_host(relay: String, fid_hex: String, port: u16, end: Arc<AtomicBool>) {
    let Some(fid) = unhex4(&fid_hex) else { return };
    let Some(raddr) = relay.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return };
    let (Ok(rs), Ok(ls)) = (UdpSocket::bind("0.0.0.0:0"), UdpSocket::bind("127.0.0.1:0")) else { return };
    if ls.connect(("127.0.0.1", port)).is_err() {
        return;
    }
    let hello = [fid[0], fid[1], fid[2], fid[3], b'H'];
    let _ = rs.send_to(&hello, raddr);
    let _ = rs.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = ls.set_read_timeout(Some(Duration::from_millis(300)));
    let (Ok(rs2), Ok(ls2)) = (rs.try_clone(), ls.try_clone()) else { return };
    let end2 = end.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while !end2.load(Ordering::Relaxed) {
            if let Ok((n, _)) = rs2.recv_from(&mut buf) {
                if n > 5 {
                    let _ = ls2.send(&buf[5..n]);
                }
            }
        }
    });
    let mut buf = [0u8; 2048];
    let mut last = Instant::now();
    while !end.load(Ordering::Relaxed) {
        if let Ok(n) = ls.recv(&mut buf[5..]) {
            buf[..4].copy_from_slice(&fid);
            buf[4] = b'H';
            let _ = rs.send_to(&buf[..5 + n], raddr);
        }
        if last.elapsed() > Duration::from_secs(10) {
            last = Instant::now();
            let _ = rs.send_to(&hello, raddr);
        }
    }
}

/// Гость: слушает локальный UDP-порт (как у хозяина) и гонит пакеты через сервер к хозяину.
fn udp_guest(token: String, relay: String, host: String, port: u16, stop: Arc<AtomicBool>) {
    let Some(raddr) = relay.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return };
    let bind = std::env::var("NOXEL_GUEST_BIND").unwrap_or_else(|_| "127.0.0.1".into()); // для проверки на одном компьютере хозяин и гость слушают разные адреса
    let Ok(local) = UdpSocket::bind((bind.as_str(), port)) else {
        eprintln!("[комната] локальный UDP-порт {port} занят, голосовой чат не подключён");
        return;
    };
    let _ = local.set_read_timeout(Some(Duration::from_millis(250)));
    let mut buf = [0u8; 2048];
    // активный поток: (адрес клиента, сокет к серверу, id потока, управляющее TCP-соединение)
    let mut cur: Option<(std::net::SocketAddr, UdpSocket, [u8; 4], TcpStream)> = None;
    while !stop.load(Ordering::Relaxed) {
        let Ok((n, from)) = local.recv_from(&mut buf[5..]) else { continue };
        if cur.as_ref().map(|c| c.0 != from).unwrap_or(true) {
            cur = None; // закрывает прежний поток
            let Ok(mut ctl) = connect(&relay) else { continue };
            let Ok((t, _)) = ticket(&token) else { continue };
            if ctl.write_all(format!("UDP {t} {host} {port}\n").as_bytes()).is_err() {
                continue;
            }
            let _ = ctl.set_read_timeout(Some(Duration::from_secs(10)));
            let line = read_line(&mut ctl).unwrap_or_default();
            let Some(fid) = line.strip_prefix("OK ").and_then(unhex4) else { continue };
            let _ = ctl.set_read_timeout(None);
            let Ok(rs) = UdpSocket::bind("0.0.0.0:0") else { continue };
            let _ = rs.set_read_timeout(Some(Duration::from_millis(300)));
            let (Ok(rs2), Ok(l2)) = (rs.try_clone(), local.try_clone()) else { continue };
            let (st2, client) = (stop.clone(), from);
            thread::spawn(move || {
                let mut b = [0u8; 2048];
                // поток живёт, пока открыт его сокет у основного цикла; при замене рабочий поток выходит по ошибке сокета
                while !st2.load(Ordering::Relaxed) {
                    match rs2.recv_from(&mut b) {
                        Ok((k, _)) if k > 5 => {
                            let _ = l2.send_to(&b[5..k], client);
                        }
                        Ok(_) => {}
                        Err(ref x) if matches!(x.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(_) => break,
                    }
                }
            });
            cur = Some((from, rs, fid, ctl));
        }
        if let Some((_, rs, fid, _)) = &cur {
            buf[..4].copy_from_slice(fid);
            buf[4] = b'G';
            let _ = rs.send_to(&buf[..5 + n], raddr);
        }
    }
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
    let mut flows: HashMap<String, Arc<AtomicBool>> = HashMap::new();
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        if let Some(id) = line.strip_prefix("CONN ") {
            let (relay, id) = (relay.clone(), id.trim().to_string());
            thread::spawn(move || host_session(relay, id, port));
        } else if let Some(rest) = line.strip_prefix("UDPFLOW ") {
            let mut it = rest.split_whitespace();
            if let (Some(fid), Some(p)) = (it.next(), it.next().and_then(|x| x.parse::<u16>().ok())) {
                if room_ports().contains(&p) {
                    let end = Arc::new(AtomicBool::new(false));
                    flows.insert(fid.to_string(), end.clone());
                    let (relay, fid) = (relay.clone(), fid.to_string());
                    thread::spawn(move || udp_host(relay, fid, p, end));
                }
            }
        } else if let Some(fid) = line.strip_prefix("UDPEND ") {
            if let Some(f) = flows.remove(fid.trim()) {
                f.store(true, Ordering::Relaxed);
            }
        }
    }
    for f in flows.values() {
        f.store(true, Ordering::Relaxed);
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
    // голосовые и другие UDP-порты, которые хозяин объявил друзьям
    if let Ok(fr) = crate::social::friends(token) {
        let ports: Vec<u16> = fr["friends"].as_array().and_then(|a| a.iter().find(|f| f["name"].as_str().map(|n| n.eq_ignore_ascii_case(host_name)).unwrap_or(false))).and_then(|f| f["ports"].as_array().cloned()).unwrap_or_default().iter().filter_map(|p| p.as_u64().map(|p| p as u16)).collect();
        for p in ports {
            let (t, r, h, st) = (token.to_string(), relay.clone(), host_name.to_string(), stop.clone());
            thread::spawn(move || udp_guest(t, r, h, p, st));
        }
    }
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
            let _ = crate::social::pack_publish(token, profile_id);
            let _ = crate::social::presence(token, None, None);
            Ok(serde_json::json!({"port": port, "how": "auto"}))
        }
        Some("error:no-world") => Err("Зайди в одиночный мир (не на сервер) и повтори".into()),
        other => {
            // версии без читаемых имён классов: пользователь открывает мир сам (Esc → «Открыть для сети»), порт берём из лога
            let lan = LAN_PORT.load(Ordering::Relaxed);
            if lan > 0 {
                host_open(token, lan)?;
                let _ = crate::social::pack_publish(token, profile_id);
                let _ = crate::social::presence(token, None, None);
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
