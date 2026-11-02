//! Разбор вылета игры: по логу и отчёту о краше понятным языком объясняет,
//! что случилось и какой мод виноват, и предлагает, что сделать.
use crate::mc::inst_dir;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, io::Read, path::Path};

/// Мод, к которому относится класс: ищем пакет внутри jar-файлов профиля.
fn mods_by_package(dir: &Path) -> Vec<(String, String, Vec<String>)> {
    // (файл, id мода, пакеты верхнего уровня вида com/foo/bar)
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(dir.join("mods")) else { return out };
    for f in rd.flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if !name.ends_with(".jar") {
            continue;
        }
        let Ok(file) = fs::File::open(f.path()) else { continue };
        let Ok(mut z) = zip::ZipArchive::new(file) else { continue };
        let mut id = String::new();
        for meta in ["fabric.mod.json", "quilt.mod.json"] {
            if let Ok(mut ent) = z.by_name(meta) {
                let mut s = String::new();
                let _ = ent.read_to_string(&mut s);
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    id = v["id"].as_str().or(v["quilt_loader"]["id"].as_str()).unwrap_or("").to_string();
                }
                break;
            }
        }
        if id.is_empty() {
            for meta in ["META-INF/mods.toml", "META-INF/neoforge.mods.toml"] {
                if let Ok(mut ent) = z.by_name(meta) {
                    let mut s = String::new();
                    let _ = ent.read_to_string(&mut s);
                    id = s.lines().find_map(|l| l.trim().strip_prefix("modId")).and_then(|r| r.split('"').nth(1)).unwrap_or("").to_string();
                    break;
                }
            }
        }
        let mut pk: Vec<String> = vec![];
        for n in z.file_names() {
            if n.ends_with(".class") && !n.starts_with("META-INF") {
                let parts: Vec<&str> = n.split('/').collect();
                if parts.len() >= 4 {
                    let p = parts[..3].join("/");
                    if !pk.contains(&p) {
                        pk.push(p);
                    }
                }
            }
        }
        out.push((name, id, pk));
    }
    out
}

const SKIP: &[&str] = &["java/", "javax/", "jdk/", "sun/", "net/minecraft/", "com/mojang/", "net/fabricmc/", "org/spongepowered/", "org/lwjgl/", "cpw/mods/", "net/minecraftforge/", "net/neoforged/", "org/quiltmc/", "com/google/", "io/netty/", "it/unimi/", "org/apache/", "org/objectweb/", "com/llamalad7/"];

fn newest_crash_report(dir: &Path, since: u64) -> Option<String> {
    let rd = fs::read_dir(dir.join("crash-reports")).ok()?;
    let best = rd.flatten().filter_map(|f| f.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, f.path()))).max_by_key(|x| x.0)?;
    let age = best.0.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    if age + 5 < since {
        return None;
    }
    fs::read_to_string(best.1).ok()
}

/// Разбирает вылет. `lines` — хвост вывода игры, `started` — время запуска (юникс-секунды).
pub fn analyze(profile_id: &str, lines: &[String], code: i32, started: u64) -> Value {
    let dir = inst_dir(profile_id);
    let report = newest_crash_report(&dir, started).unwrap_or_default();
    let log = lines.join("\n");
    let all = format!("{report}\n{log}");
    let low = all.to_lowercase();
    let mut culprits: Vec<String> = vec![];
    let (mut title, mut advice) = (String::from("Игра вылетела"), String::new());
    let mut missing: Vec<String> = vec![];

    // 1. нехватка памяти и Java
    if low.contains("java.lang.outofmemoryerror") {
        title = "Игре не хватило памяти".into();
        advice = "Увеличь память для игры в настройках (ползунок «Память»). Для сборок со множеством модов нужно 4–8 ГБ.".into();
    } else if low.contains("could not reserve enough space") || low.contains("could not create the java virtual machine") {
        title = "Java не смогла выделить память".into();
        advice = "Уменьши память для игры в настройках: система не даёт столько свободной оперативной памяти.".into();
    } else if low.contains("unsupportedclassversionerror") || low.contains("has been compiled by a more recent version of the java runtime") {
        title = "Не та версия Java".into();
        advice = "Очисти путь к Java в настройках: лаунчер сам поставит нужную версию для этой игры.".into();
    } else if low.contains("glfw error") || low.contains("pixel format not accelerated") || low.contains("no opengl context") || low.contains("the driver does not appear to support opengl") {
        title = "Проблема с видеодрайвером".into();
        advice = "Обнови драйвер видеокарты. На ноутбуке с двумя видеокартами запусти игру на дискретной.".into();
    }

    // 2. Fabric/Quilt: не хватает модов или они несовместимы
    for l in all.lines() {
        let t = l.trim();
        // «- Mod 'Sodium Extra' (sodium-extra) 0.5 requires any version of sodium, which is missing!»
        if let Some(i) = t.find(" requires ") {
            if t.contains("which is missing") || t.contains("but only the wrong version is present") {
                let who = t[..i].trim_start_matches("- ").trim_start_matches("Mod ").to_string();
                let what = t[i + 10..].split(", which").next().unwrap_or("").replace("any version of ", "").replace("version ", "");
                missing.push(format!("{who} → нужен {what}"));
            }
        }
        if t.contains("is incompatible with") || t.contains("Incompatible mod set") {
            culprits.push(t.trim_start_matches("- ").chars().take(160).collect());
        }
        if let Some(r) = t.strip_prefix("Suspected Mod").or_else(|| t.strip_prefix("Suspected Mods:")) {
            let r = r.trim_start_matches(|c| c == 's' || c == ':').trim();
            if !r.is_empty() && !r.eq_ignore_ascii_case("none") {
                culprits.push(r.to_string());
            }
        }
        // ошибка миксина: «Mixin [sodium.mixins.json:...] from mod sodium» / «from mod xyz failed injection»
        if t.contains("Mixin") && t.contains("from mod ") {
            if let Some(m) = t.split("from mod ").nth(1) {
                let id = m.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).next().unwrap_or("").to_string();
                if !id.is_empty() {
                    culprits.push(id);
                }
            }
        }
        if t.contains("Duplicate mod") || t.contains("Found duplicate mods") || t.contains("Mod ID") && t.contains("duplicate") {
            title = "Один мод стоит дважды".into();
            advice = "Удали одну из копий мода (обычно в папке модов лежат две версии).".into();
            culprits.push(t.chars().take(160).collect());
        }
    }
    if !missing.is_empty() {
        title = "Не хватает модов-зависимостей".into();
        advice = "Поставь недостающие моды через каталог (вкладка «Моды» → «Каталог») или убери мод, которому они нужны.".into();
    }

    // 3. по стеку вызовов: первый «чужой» пакет указывает на мод
    let mods = mods_by_package(&dir);
    let mut hits: HashMap<String, usize> = HashMap::new();
    for l in all.lines() {
        let t = l.trim();
        let Some(frame) = t.strip_prefix("at ") else { continue };
        let cls = frame.split('(').next().unwrap_or("").replace('.', "/");
        if SKIP.iter().any(|s| cls.starts_with(s)) {
            continue;
        }
        for (file, _, pk) in &mods {
            if pk.iter().any(|p| cls.starts_with(p.as_str())) {
                *hits.entry(file.clone()).or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<(String, usize)> = hits.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    // id из сообщений превращаем в файлы
    let mut files: Vec<String> = ranked.iter().take(3).map(|x| x.0.clone()).collect();
    for c in &culprits {
        let cl = c.to_lowercase();
        for (file, id, _) in &mods {
            if !id.is_empty() && (cl == id.to_lowercase() || cl.contains(&format!("({})", id.to_lowercase())) || cl.starts_with(&id.to_lowercase())) && !files.contains(file) {
                files.push(file.clone());
            }
        }
    }
    files.truncate(4);
    if title == "Игра вылетела" && !files.is_empty() {
        title = "Игру уронил мод".into();
        advice = "Попробуй выключить этот мод или обновить его. Если вылет после обновления, верни профиль к точке восстановления.".into();
    }
    if advice.is_empty() {
        advice = if code == 0 { String::new() } else { "Открой лог, чтобы увидеть подробности. Если вылет начался после установки или обновления модов, верни профиль к точке восстановления.".into() };
    }
    // строка с исключением, понятная разработчику мода
    let exc = all.lines().map(str::trim).find(|l| (l.contains("Exception") || l.contains("Error:")) && !l.starts_with("at ") && l.len() < 300).unwrap_or("").to_string();
    json!({"title": title, "advice": advice, "mods": files, "missing": missing, "exception": exc, "report": !report.is_empty(), "code": code})
}
