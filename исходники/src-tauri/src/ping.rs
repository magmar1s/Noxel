//! Мониторинг серверов: пинг, онлайн и MOTD по обычному протоколу списка серверов Minecraft.
use crate::util::*;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

fn varint(mut v: u32, out: &mut Vec<u8>) {
    loop {
        let mut b = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn read_varint(r: &mut impl Read) -> R<u32> {
    let mut n = 0u32;
    for i in 0..5 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b).map_err(e)?;
        n |= ((b[0] & 0x7F) as u32) << (7 * i);
        if b[0] & 0x80 == 0 {
            return Ok(n);
        }
    }
    Err("неверный ответ сервера".into())
}

fn packet(id: u8, body: &[u8]) -> Vec<u8> {
    let mut p = vec![id];
    p.extend(body);
    let mut o = vec![];
    varint(p.len() as u32, &mut o);
    o.extend(p);
    o
}

fn motd(v: &Value) -> String {
    let mut s = String::new();
    match v {
        Value::String(t) => s.push_str(t),
        Value::Object(_) => {
            if let Some(t) = v["text"].as_str() {
                s.push_str(t);
            }
            for x in v["extra"].as_array().into_iter().flatten() {
                s.push_str(&motd(x));
            }
        }
        _ => {}
    }
    // убираем цветовые коды §x
    let mut out = String::new();
    let mut skip = false;
    for c in s.chars() {
        if skip {
            skip = false;
        } else if c == '§' {
            skip = true;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn status(host_port: &str) -> Value {
    match status_inner(host_port) {
        Ok(v) => v,
        Err(x) => json!({"online": false, "error": x}),
    }
}

fn status_inner(hp: &str) -> R<Value> {
    let (host, port) = match hp.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(25565)),
        None => (hp, 25565),
    };
    let addr = (host, port).to_socket_addrs().map_err(|_| "адрес не найден")?.next().ok_or("адрес не найден")?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|_| "сервер не отвечает")?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    s.set_write_timeout(Some(Duration::from_secs(3))).ok();
    let mut hs = vec![];
    varint(765, &mut hs);
    varint(host.len() as u32, &mut hs);
    hs.extend(host.as_bytes());
    hs.extend(port.to_be_bytes());
    varint(1, &mut hs);
    s.write_all(&packet(0, &hs)).map_err(e)?;
    s.write_all(&packet(0, &[])).map_err(e)?;
    read_varint(&mut s)?;
    read_varint(&mut s)?;
    let n = read_varint(&mut s)? as usize;
    if n > 1_500_000 {
        return Err("слишком большой ответ".into());
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).map_err(e)?;
    let v: Value = serde_json::from_slice(&buf).map_err(e)?;
    let t = Instant::now();
    let ping = s.write_all(&packet(1, &[0; 8])).ok().and_then(|_| {
        read_varint(&mut s).ok()?;
        read_varint(&mut s).ok()?;
        let mut b = [0u8; 8];
        s.read_exact(&mut b).ok()?;
        Some(t.elapsed().as_millis() as u64)
    });
    Ok(json!({
        "online": true, "ping": ping, "players": v["players"]["online"], "max": v["players"]["max"],
        "version": v["version"]["name"], "motd": motd(&v["description"]),
    }))
}
