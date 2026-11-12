//! Скины и плащи: разбор PNG, приведение к формату 64x64, клиент Noxel API.
use crate::util::*;
use base64::Engine;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

/// Публичный адрес API. Здесь только адрес сервера: никаких паролей и ключей.
pub const API: &str = "https://mr1s.site/noxel/v1";

pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>, // RGBA
}

pub fn decode(bytes: &[u8]) -> R<Img> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|_| "не удалось прочитать PNG")?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|_| "не удалось прочитать PNG")?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data = &buf[..info.buffer_size()];
    let px: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("неподдерживаемый формат PNG".into()),
    };
    Ok(Img { w, h, px })
}

pub fn encode(img: &Img) -> R<Vec<u8>> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, img.w as u32, img.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(e)?;
        w.write_image_data(&img.px).map_err(e)?;
    }
    Ok(out)
}

/// Как `NativeImage.copyRect` в игре: копирует блок со сдвигом и зеркалом по X.
fn copy_rect(img: &mut Img, x: usize, y: usize, xo: isize, yo: isize, w: usize, h: usize) {
    for dy in 0..h {
        for dx in 0..w {
            let s = ((y + dy) * img.w + x + dx) * 4;
            let tx = (x as isize + xo + (w - 1 - dx) as isize) as usize;
            let ty = (y as isize + yo + dy as isize) as usize;
            let t = (ty * img.w + tx) * 4;
            let p = [img.px[s], img.px[s + 1], img.px[s + 2], img.px[s + 3]];
            img.px[t..t + 4].copy_from_slice(&p);
        }
    }
}

fn no_alpha(img: &mut Img, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1 {
        for x in x0..x1 {
            img.px[(y * img.w + x) * 4 + 3] = 255;
        }
    }
}

/// Приводит скин к 64x64 (старые 64x32 достраиваются, как это делает сама игра) и определяет тип рук.
pub fn normalize_skin(bytes: &[u8]) -> R<(Vec<u8>, String)> {
    let src = decode(bytes)?;
    let mut img = match (src.w, src.h) {
        (64, 64) => src,
        (64, 32) => {
            let mut px = vec![0u8; 64 * 64 * 4];
            px[..src.px.len()].copy_from_slice(&src.px);
            let mut im = Img { w: 64, h: 64, px };
            for (x, y, xo, yo, w, h) in [
                (4, 16, 16, 32, 4, 4), (8, 16, 16, 32, 4, 4), (0, 20, 24, 32, 4, 12), (4, 20, 16, 32, 4, 12),
                (8, 20, 8, 32, 4, 12), (12, 20, 16, 32, 4, 12), (44, 16, -8, 32, 4, 4), (48, 16, -8, 32, 4, 4),
                (40, 20, 0, 32, 4, 12), (44, 20, -8, 32, 4, 12), (48, 20, -16, 32, 4, 12), (52, 20, -8, 32, 4, 12),
            ] {
                copy_rect(&mut im, x, y, xo, yo, w, h);
            }
            im
        }
        (w, h) => return Err(format!("скин должен быть 64x64 или 64x32, а у файла {w}x{h}")),
    };
    // тонкие руки: крайние колонки правой руки полностью прозрачны
    let slim = (20..32).all(|y| (54..56).all(|x| img.px[(y * 64 + x) * 4 + 3] == 0));
    no_alpha(&mut img, 0, 0, 32, 16);
    no_alpha(&mut img, 0, 16, 64, 32);
    no_alpha(&mut img, 16, 48, 48, 64);
    Ok((encode(&img)?, if slim { "slim" } else { "classic" }.into()))
}

pub fn check_cape(bytes: &[u8]) -> R<()> {
    let im = decode(bytes)?;
    if matches!((im.w, im.h), (64, 32) | (22, 17) | (46, 22)) {
        Ok(())
    } else {
        Err(format!("плащ должен быть 64x32, 22x17 или 46x22, а у файла {}x{}", im.w, im.h))
    }
}

pub fn sha256(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

// ── локальное хранилище: файл = sha256 содержимого ──

fn dir() -> PathBuf {
    data_dir().join("skins")
}

pub fn store(bytes: &[u8]) -> R<String> {
    let h = sha256(bytes);
    let p = dir().join(format!("{h}.png"));
    if !p.exists() {
        fs::create_dir_all(dir()).map_err(e)?;
        fs::write(&p, bytes).map_err(e)?;
    }
    Ok(h)
}

pub fn load(hash: &str) -> R<Vec<u8>> {
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("неверный хеш".into());
    }
    fs::read(dir().join(format!("{hash}.png"))).map_err(|_| "файл скина не найден".to_string())
}

pub fn data_url(hash: &str) -> R<String> {
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(load(hash)?)))
}

pub fn fetch_png(url: &str) -> R<Vec<u8>> {
    use std::io::Read;
    let mut v = vec![];
    agent().get(url).call().map_err(e)?.into_reader().take(200_000).read_to_end(&mut v).map_err(e)?;
    Ok(v)
}

// ── клиент Noxel API ──

pub enum Body {
    None,
    Json(Value),
    Bytes(Vec<u8>),
}

#[cfg(test)]
fn api_base() -> String {
    std::env::var("T_API").unwrap_or_else(|_| API.into())
}
#[cfg(not(test))]
pub fn call(method: &str, path: &str, token: Option<&str>, body: Body) -> R<Value> {
    let _ = token;
    match body { Body::None => {}, Body::Json(value) => { drop(value); }, Body::Bytes(bytes) => { drop(bytes); } }
    if method == "GET" {
        let name = match path { "/launcher" => Some("launcher"), "/news" => Some("news"), "/servers" => Some("servers"), "/recommended" => Some("recommended"), _ => None };
        if let Some(name) = name { return get_json(&format!("https://raw.githubusercontent.com/magmar1s/Noxel/main/distribution/{name}.json")); }
    }
    Err("Серверные функции Noxel отключены в GitHub-версии. Используй локальный или Microsoft-аккаунт.".into())
}
#[cfg(test)]
pub fn call(method: &str, path: &str, token: Option<&str>, body: Body) -> R<Value> {

    let mut req = agent().request(method, &format!("{}{path}", api_base()));
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let res = match body {
        Body::None => req.call(),
        Body::Json(v) => req.send_json(v),
        Body::Bytes(b) => req.set("Content-Type", "application/octet-stream").send_bytes(&b),
    };
    match res {
        Ok(r) => r.into_json().map_err(e),
        Err(ureq::Error::Status(c, r)) => {
            let v: Value = r.into_json().unwrap_or(json!({}));
            Err(v["error"].as_str().map(String::from).unwrap_or_else(|| format!("сервер Noxel ответил {c}")))
        }
        Err(x) => Err(format!("нет связи с сервером Noxel: {x}")),
    }
}
