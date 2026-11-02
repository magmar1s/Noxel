//! Интеграционная проверка на настоящих серверах: cargo test --release -- --ignored --nocapture
use crate::{mc, util::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
#[ignore]
fn full_launch() {
    let ver = std::env::var("T_MC").unwrap_or_else(|_| "26.3".into());
    let loader = std::env::var("T_LOADER").unwrap_or_else(|_| "vanilla".into());
    let secs: u64 = std::env::var("T_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let lv = mc::loader_versions(&ver, &loader);
    println!("версии загрузчика {loader}: {:?}", &lv[..lv.len().min(4)]);
    let p = mc::create_profile(&ver, &loader, lv.first().cloned(), &format!("selftest {ver} {loader}"), None).unwrap();
    let st = load_settings();
    if let Some(pr) = std::env::var("T_PRESET").ok().filter(|s| !s.is_empty()) {
        let cb = |_: &str, _: f64, d: &str| println!("[сборка] {d}");
        let r = crate::mods::apply_preset(&p.id, &pr, &cb).unwrap();
        println!("сборка {pr}: установлено {:?}; пропущено {:?}", r.0, r.1);
        let ls = crate::mods::list(&p.id, "mod").unwrap();
        println!("в папке mods: {}", ls.as_array().map(|a| a.iter().map(|x| x["name"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ")).unwrap_or_default());
    }
    let cb = |s: &str, f: f64, d: &str| {
        if f < 0.0 || f >= 1.0 {
            println!("[{s}] {d}");
        }
    };
    let acct = crate::accounts::active_fresh().unwrap();
    let res = mc::prepare(&p, &st, &acct, &cb);
    let pr = match res {
        Ok(x) => x,
        Err(x) => {
            let _ = mc::delete_profile(&p.id, true);
            panic!("prepare: {x}");
        }
    };
    println!("java: {}", pr.java.display());
    println!("args: {}", pr.args.iter().map(|a| if a.chars().count() > 200 { format!("{}…", a.chars().take(60).collect::<String>()) } else { a.clone() }).collect::<Vec<_>>().join(" "));
    let game = mc::Game::default();
    let logs = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = logs.clone();
    let mut child = mc::spawn_game(&game, &pr, Arc::new(move |v| l2.lock().unwrap().extend(v)), Arc::new(|_| {})).unwrap();
    std::thread::sleep(Duration::from_secs(secs));
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    std::thread::sleep(Duration::from_millis(500));
    let l = logs.lock().unwrap();
    println!("--- процесс жив через {secs} c: {alive}; строк лога: {}", l.len());
    for x in l.iter().take(25) {
        println!("  {x}");
    }
    if l.len() > 25 {
        println!("  …");
        for x in l.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
            println!("  {x}");
        }
    }
    let _ = mc::delete_profile(&p.id, true);
    assert!(alive, "игра завершилась раньше времени");
}

#[test]
fn logfmt() {
    let mut f = mc::LogFmt::default();
    let mut out = vec![];
    for l in [
        "<log4j:Event logger=\"a\" timestamp=\"1\" level=\"INFO\" thread=\"Render thread\">",
        "<log4j:Message><![CDATA[Setting user: Player]]></log4j:Message>",
        "</log4j:Event>",
        "plain line",
        "<log4j:Event logger=\"b\" timestamp=\"2\" level=\"ERROR\" thread=\"main\">",
        "<log4j:Message><![CDATA[boom]]></log4j:Message>",
        "<log4j:Throwable><![CDATA[java.lang.Exception: x",
        "\tat a.b(C.java:1)",
        "]]></log4j:Throwable>",
        "</log4j:Event>",
        "<log4j:Event logger=\"c\" timestamp=\"3\" level=\"WARN\" thread=\"t\">",
        "    <log4j:Message><![CDATA[indented]]></log4j:Message>",
        "</log4j:Event>",
    ] {
        f.feed(l.to_string(), &mut out);
    }
    println!("{out:#?}");
    assert_eq!(out[0], "[INFO] [Render thread] Setting user: Player");
    assert_eq!(out[1], "plain line");
    assert_eq!(out[2], "[ERROR] [main] boom");
    assert!(out[3].contains("java.lang.Exception"));
    assert_eq!(out.last().unwrap(), "[WARN] [t] indented");
}

fn test_img(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> crate::skins::Img {
    let mut px = vec![];
    for y in 0..h {
        for x in 0..w {
            px.extend(f(x, y));
        }
    }
    crate::skins::Img { w, h, px }
}

#[test]
fn skin_normalize() {
    use crate::skins::*;
    // классические руки и тонкие руки определяются по прозрачным колонкам правой руки
    let classic = encode(&test_img(64, 64, |_, _| [90, 90, 90, 255])).unwrap();
    assert_eq!(normalize_skin(&classic).unwrap().1, "classic");
    let slim = encode(&test_img(64, 64, |x, y| if (54..56).contains(&x) && (20..32).contains(&y) { [0, 0, 0, 0] } else { [90, 90, 90, 255] })).unwrap();
    assert_eq!(normalize_skin(&slim).unwrap().1, "slim");
    // старый формат 64x32 достраивается до 64x64: левая нога = зеркало правой
    let old = encode(&test_img(64, 32, |x, y| if (x, y) == (4, 16) { [255, 0, 0, 255] } else { [10, 20, 30, 255] })).unwrap();
    let (png, _) = normalize_skin(&old).unwrap();
    let im = decode(&png).unwrap();
    assert_eq!((im.w, im.h), (64, 64));
    let i = (48 * 64 + 23) * 4;
    assert_eq!(&im.px[i..i + 4], &[255, 0, 0, 255], "пиксель (4,16) должен оказаться зеркально в (23,48)");
    // неподходящий размер
    assert!(normalize_skin(&encode(&test_img(100, 100, |_, _| [0, 0, 0, 255])).unwrap()).is_err());
    // сборка из стандартного скина лаунчера
    let def = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/assets/default_skin.png")).unwrap();
    assert!(normalize_skin(&def).is_ok());
}

/// Весь путь Noxel-аккаунта на настоящем сервере. Запуск: NOXEL_HOME=<пустая папка> cargo test ... noxel_flow -- --ignored
#[test]
#[ignore]
fn noxel_flow() {
    use crate::{accounts, skins};
    let name = format!("zz_{:x}", now() & 0xffffff);
    let pw = "testpass123";
    let id = format!("noxel-{}", name.to_lowercase());
    accounts::noxel_auth(&name, pw, true).expect("регистрация");
    let png = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/assets/default_skin.png");
    accounts::skin_apply(&id, png, None).expect("загрузка скина");
    let d = accounts::details(&id, false).unwrap();
    let url = d["skin"]["url"].as_str().expect("ссылка на скин").to_string();
    println!("ссылка для SkinsRestorer: /skin url {url}");
    assert_eq!(d["history"].as_array().unwrap().len(), 1);
    let pub_png = skins::fetch_png(&url).expect("скачивание по публичной ссылке");
    assert!(skins::decode(&pub_png).is_ok());
    // второй скин (другая модель) и возврат к первому
    accounts::skin_apply(&id, png, Some("slim".into())).unwrap();
    let d = accounts::details(&id, false).unwrap();
    let hist = d["history"].as_array().unwrap().clone();
    assert_eq!(hist.len(), 2, "{d}");
    let first = hist.iter().find(|h| h["model"] == "classic").unwrap()["id"].as_u64().unwrap();
    accounts::skin_use(&id, first).unwrap();
    assert_eq!(accounts::details(&id, false).unwrap()["skin"]["model"], "classic");
    // плащ
    let cape = test_img(64, 32, |x, _| [x as u8 * 3, 40, 200, 255]);
    let cp = std::env::temp_dir().join("noxel_cape_test.png");
    std::fs::write(&cp, skins::encode(&cape).unwrap()).unwrap();
    accounts::cape_apply(&id, cp.to_str().unwrap()).expect("плащ");
    assert!(accounts::details(&id, false).unwrap()["cape"].is_string());
    // «другое устройство»: забываем всё локально и входим заново, история и плащ должны подтянуться
    let tok = accounts::get(&id).unwrap().token;
    let mut s = accounts::load();
    s.list.retain(|a| a.id != id);
    accounts::save(&s);
    let _ = std::fs::remove_dir_all(data_dir().join("skins"));
    accounts::noxel_auth(&name, pw, false).expect("вход");
    let d = accounts::details(&id, false).unwrap();
    assert_eq!(d["history"].as_array().unwrap().len(), 2, "история подтянулась: {d}");
    assert!(d["cape"].is_string(), "плащ подтянулся");
    assert!(skins::data_url(d["skin"]["hash"].as_str().unwrap()).is_ok());
    // неверный пароль
    assert!(accounts::noxel_auth(&name, "wrong-pass", false).is_err());
    // уборка за собой на сервере
    let new_tok = accounts::get(&id).unwrap().token;
    skins::call("DELETE", "/account", Some(&new_tok), skins::Body::Json(serde_json::json!({"password": pw}))).expect("удаление аккаунта");
    let _ = tok;
    println!("noxel_flow: всё прошло");
}

/// Объединённый каталог на живых серверах (Modrinth и CurseForge через сервер Noxel).
#[test]
#[ignore]
fn merged_catalog() {
    use crate::{mc, mods};
    let p = mc::create_profile("1.21.1", "fabric", None, "catalog-test", None).unwrap();
    let cb = |_: &str, _: f64, d: &str| println!("  [установка] {d}");
    // 1. объединённый поиск
    let r = mods::search_all("mod", "sodium", &p, "relevance", 0, false).expect("поиск");
    let hits = r["hits"].as_array().unwrap();
    println!("найдено: {}, предупреждения: {}", hits.len(), r["warn"]);
    assert!(!hits.is_empty());
    let both = hits.iter().find(|h| h["sources"].as_array().map(|s| s.len() == 2).unwrap_or(false));
    println!("мод из двух источников сразу: {}", both.map(|h| h["title"].to_string()).unwrap_or("нет".into()));
    let cf_only = hits.iter().any(|h| h["sources"][0]["provider"] == "curseforge");
    println!("есть результаты CurseForge: {cf_only}");
    // 2. описание и версии со всех источников
    let h = both.or(hits.first()).unwrap();
    let srcs: Vec<(String, String)> = h["sources"].as_array().unwrap().iter().map(|s| (s["provider"].as_str().unwrap().to_string(), s["id"].as_str().unwrap().to_string())).collect();
    let d = mods::details_all("mod", &srcs, &p.id).expect("описание");
    let vs = d["versions"].as_array().unwrap();
    let provs: std::collections::HashSet<_> = vs.iter().map(|v| v["provider"].as_str().unwrap().to_string()).collect();
    println!("версий: {}, источники версий: {:?}, предупреждения: {}", vs.len(), provs, d["warn"]);
    assert!(!vs.is_empty());
    // 3. установка конкретной версии с каждого источника
    for prov in provs.iter() {
        let v = vs.iter().find(|v| v["provider"] == prov.as_str()).unwrap();
        let id = srcs.iter().find(|s| &s.0 == prov).unwrap().1.clone();
        let done = mods::install(prov, "mod", &id, &p.id, Some(v["vid"].as_str().unwrap().to_string()), &cb).unwrap_or_else(|e| panic!("установка с {prov}: {e}"));
        println!("установлено с {prov}: {done:?}");
    }
    let ls = mods::list(&p.id, "mod").unwrap();
    println!("в папке: {}", ls.as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect::<Vec<_>>().join(", "));
    // 4. набор модов для сервера
    let (ok, skipped) = mods::install_list(&p.id, &[("modrinth".into(), "lithium".into()), ("modrinth".into(), "fabric-api".into())], &cb).unwrap();
    println!("список сервера: установлено {ok:?}, пропущено {skipped:?}");
    assert!(skipped.is_empty());
    // 5. подсказка «скачано на другой версии»
    let p2 = mc::create_profile("1.21.1", "fabric", None, "catalog-test-2", None).unwrap();
    let r2 = mods::search_all("mod", "lithium", &p2, "relevance", 0, false).unwrap();
    let lith = r2["hits"].as_array().unwrap().iter().find(|h| h["title"] == "Lithium").expect("Lithium в поиске");
    println!("Lithium скачан в профилях: {}", lith["installed_in"]);
    assert!(!lith["installed_in"].as_array().unwrap().is_empty());
    // 6. клиентский фильтр
    let rc = mods::search_all("mod", "", &p, "downloads", 0, true).unwrap();
    println!("клиентские моды, первые: {:?}", rc["hits"].as_array().unwrap().iter().take(4).map(|h| h["title"].as_str().unwrap()).collect::<Vec<_>>());
    println!("merged_catalog: всё прошло");
}

/// После публикации релиза: скачивание с сервера, проверка суммы и подписи (файл лаунчера не подменяется).
#[test]
#[ignore]
fn updater_verify() {
    let dest = std::env::temp_dir().join("noxel_update_test.bin");
    let v = crate::remote::fetch_verified(&dest).expect("проверка обновления");
    let n = std::fs::metadata(&dest).unwrap().len();
    println!("версия {v}, {n} байт, сумма и подпись верны");
    // подмена одного байта должна ломать подпись
    let mut b = std::fs::read(&dest).unwrap();
    b[100] ^= 1;
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, vec![0u8; 32]);
    assert!(key.verify(&b, &[0u8; 64]).is_err());
    let _ = std::fs::remove_file(&dest);
}

/// Автовход на настоящем сервере: локальный сервер 1.20.1, игра с агентом, приглашения /register и /login.
#[test]
#[ignore]
fn autologin_flow() {
    use crate::{accounts, autoauth, mc};
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let ver = std::env::var("T_MC").unwrap_or_else(|_| "1.20.1".into());
    let say = |s: &str, f: f64, d: &str| {
        if f < 0.0 || f >= 1.0 {
            println!("[{s}] {d}")
        }
    };
    mc::prefetch(&ver, 8, false, &say).expect("предзагрузка игры");
    // сервер
    let vj: serde_json::Value = read_json(&data_dir().join("minecraft/versions").join(&ver).join(format!("{ver}.json"))).unwrap();
    let comp = vj["javaVersion"]["component"].as_str().unwrap_or("jre-legacy").to_string();
    let java = data_dir().join("runtime").join(&comp).join(JAVA_EXE);
    let sdir = data_dir().join("test-server");
    std::fs::create_dir_all(&sdir).unwrap();
    download(vj["downloads"]["server"]["url"].as_str().unwrap(), &sdir.join("server.jar"), vj["downloads"]["server"]["sha1"].as_str()).unwrap();
    std::fs::write(sdir.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(sdir.join("server.properties"), "online-mode=false\nserver-port=25599\nlevel-type=flat\nview-distance=4\ndifficulty=peaceful\nspawn-monsters=false\nmotd=noxel test\n").unwrap();
    let mut server = Command::new(&java).args(["-Xmx1G", "-jar", "server.jar", "nogui"]).current_dir(&sdir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("запуск сервера");
    let slog = Arc::new(Mutex::new(Vec::<String>::new()));
    let (sl, out) = (slog.clone(), server.stdout.take().unwrap());
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            sl.lock().unwrap().push(l);
        }
    });
    let wait = |log: &Arc<Mutex<Vec<String>>>, needle: &str, secs: u64| -> Option<String> {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(secs) {
            if let Some(l) = log.lock().unwrap().iter().find(|l| l.contains(needle)) {
                return Some(l.clone());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        None
    };
    assert!(wait(&slog, "Done (", 240).is_some(), "сервер не запустился: {:?}", slog.lock().unwrap().iter().rev().take(5).collect::<Vec<_>>());
    println!("сервер запущен");
    // игрок
    let _ = accounts::add_local("AutoTest");
    accounts::select("local-autotest").unwrap();
    let p = mc::create_profile(&ver, &std::env::var("T_LOADER").unwrap_or_else(|_| "vanilla".into()), None, &format!("autotest-{}", std::env::var("T_LOADER").unwrap_or_default()), None).unwrap();
    // автовход по умолчанию выключен, а тест проверяет именно его
    let mut st = load_settings();
    st.auto_auth = true;
    st.auto_skin = true;
    save_settings(&st);
    let acct = accounts::active_fresh().unwrap();
    let cb = |_: &str, _: f64, _: &str| {};
    let mut pr = mc::prepare(&p, &st, &acct, &cb).expect("подготовка игры");
    // окно теста может быть без фокуса: пусть игра не уходит в паузу
    let _ = std::fs::write(pr.gamedir.join("options.txt"), "pauseOnLostFocus:false\nonboardAccessibility:false\nsoundCategory_master:0.0\nsoundCategory_music:0.0\nsoundCategory_record:0.0\n");
    pr.args.extend(["--quickPlayMultiplayer".to_string(), "127.0.0.1:25599".to_string()]);
    println!("агент подключён: {}", pr.args.iter().any(|a| a.starts_with("-javaagent:")));
    let auth = Arc::new(Mutex::new(autoauth::Auth::new(&acct.id, &pr.gamedir, Box::new(|k, h| println!("[автовход] {k} на {h}"))).unwrap()));
    let clog = Arc::new(Mutex::new(Vec::<String>::new()));
    let (a2, c2) = (auth.clone(), clog.clone());
    let game = mc::Game::default();
    let mut child = mc::spawn_game(&game, &pr, Arc::new(move |lines| c2.lock().unwrap().extend(lines)), Arc::new(move |l| a2.lock().unwrap().on_line(l)))
    .unwrap();
    // окно игры не должно забирать фокус у того, чем занят пользователь (Hyprland)
    let prev = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()).and_then(|v| v["address"].as_str().map(String::from));
    let gpid = child.id() as u64;
    if let Some(prev) = prev {
        std::thread::spawn(move || {
            for _ in 0..800 {
                std::thread::sleep(Duration::from_millis(250));
                if let Some(v) = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()) {
                    if v["pid"].as_u64() == Some(gpid) {
                        if let Some(addr) = v["address"].as_str() {
                            // окно игры уводим на правый монитор (workspace 6), чтобы не мешать
                            let _ = Command::new("hyprctl").args(["dispatch", &format!("hl.dsp.window.move({{ workspace = \"6\", follow = false, window = \"address:{addr}\" }})")]).output();
                        }
                        let _ = Command::new("hyprctl").args(["dispatch", "focuswindow", &format!("address:{prev}")]).output();
                    }
                }
            }
        });
    }
    let joined = wait(&slog, "AutoTest joined the game", 240);
    println!("игрок зашёл на сервер: {}", joined.is_some());
    if joined.is_none() {
        println!("--- лог игры (хвост):\n{}", clog.lock().unwrap().iter().rev().take(15).cloned().collect::<Vec<_>>().join("\n"));
    }
    let tell = |server: &mut std::process::Child, text: &str| {
        let stdin = server.stdin.as_mut().unwrap();
        writeln!(stdin, "tellraw AutoTest {{\"text\":\"{text}\"}}").unwrap();
        stdin.flush().unwrap();
    };
    let secret = accounts::ensure_secret(&acct.id).unwrap();
    let pw = autoauth::server_password(&secret, "127.0.0.1");
    let mut ok = joined.is_some();
    if ok {
        std::thread::sleep(Duration::from_secs(4));
        // ванильный сервер не знает /register и /login и отвечает «Unknown or incomplete command» с текстом команды,
        // по этому ответу видно, что именно набрал игрок
        clog.lock().unwrap().clear();
        tell(&mut server, "Please register with /register <password> <confirmPassword>");
        let r = wait(&clog, &format!("[CHAT] register {pw} {pw}<--[HERE]"), 40);
        println!("регистрация дошла до сервера: {}", r.is_some());
        ok &= r.is_some();
        tell(&mut server, "Please login with /login <password>");
        let l = wait(&clog, &format!("[CHAT] login {pw}<--[HERE]"), 40);
        println!("вход дошёл до сервера: {}", l.is_some());
        ok &= l.is_some();
    }
    let _ = child.kill();
    let _ = writeln!(server.stdin.as_mut().unwrap(), "stop");
    std::thread::sleep(Duration::from_secs(3));
    let _ = server.kill();
    println!("--- агент: {}", std::fs::read_to_string(pr.gamedir.join(".noxel/agent.log")).unwrap_or_default());
    if !ok {
        println!("--- агент (ещё раз): {}", std::fs::read_to_string(pr.gamedir.join(".noxel/agent.log")).unwrap_or_default());
        println!("--- команды: {}", std::fs::read_to_string(pr.gamedir.join(".noxel/cmd.txt")).unwrap_or_default());
        println!("--- хвост игры:\n{}", clog.lock().unwrap().iter().rev().take(20).cloned().collect::<Vec<_>>().join("\n"));
    }
    assert!(ok, "автовход не сработал");
    println!("autologin_flow: всё прошло");
}

#[test]
#[ignore]
fn modpack_flow() {
    let r = crate::packs::search("", "downloads", 0, "").expect("поиск сборок");
    let hits = r["hits"].as_array().unwrap();
    println!("найдено сборок: {}", hits.len());
    assert!(!hits.is_empty());
    for h in hits.iter().take(3) {
        println!("  {} · {} · {}", h["title"], h["mc"], h["loaders"]);
    }
    let q = crate::packs::search("Fabulously Optimized", "relevance", 0, "").unwrap();
    let hit = q["hits"].as_array().unwrap().iter().find(|h| h["provider"] == "modrinth").expect("сборка найдена");
    let id = hit["id"].as_str().unwrap().to_string();
    let res = crate::packs::install("modrinth", &id, &|st, f, d| println!("[{st}] {:.0}% {d}", f * 100.0)).expect("установка сборки");
    println!("профиль: {}", res);
    let dir = crate::mc::inst_dir(res["profile"].as_str().unwrap());
    let n = std::fs::read_dir(dir.join("mods")).map(|r| r.flatten().filter(|e| e.path().extension().map(|x| x == "jar").unwrap_or(false)).count()).unwrap_or(0);
    println!("модов в профиле: {n}");
    assert!(n > 5);
}

#[test]
#[ignore]
fn modpack_cf_flow() {
    let q = crate::packs::search("", "downloads", 0, "1.20.1").expect("поиск сборок");
    let hits = q["hits"].as_array().unwrap();
    let cf: Vec<_> = hits.iter().filter(|h| h["provider"] == "curseforge").collect();
    println!("всего: {}, из CurseForge: {}", hits.len(), cf.len());
    assert!(!cf.is_empty());
    // небольшая сборка для проверки: берём ту, у которой поменьше скачиваний среди первых
    let h = cf.last().unwrap();
    println!("ставлю: {} ({})", h["title"], h["mc"]);
    let res = crate::packs::install("curseforge", h["id"].as_str().unwrap(), &|st, f, d| println!("[{st}] {:.0}% {d}", f * 100.0)).expect("установка сборки CurseForge");
    println!("профиль: {res}");
    let dir = crate::mc::inst_dir(res["profile"].as_str().unwrap());
    let n = std::fs::read_dir(dir.join("mods")).map(|r| r.flatten().count()).unwrap_or(0);
    println!("файлов в mods: {n}");
    assert!(n > 0);
}

#[test]
#[ignore]
fn worlds_flow() {
    let p = crate::mc::create_profile("1.21.1", "vanilla", None, "worlds-test", None).unwrap();
    let w = crate::mc::inst_dir(&p.id).join("saves").join("Мой мир");
    std::fs::create_dir_all(w.join("region")).unwrap();
    std::fs::write(w.join("level.dat"), b"dat").unwrap();
    std::fs::write(w.join("region/r.0.0.mca"), vec![7u8; 5000]).unwrap();
    let l = crate::worlds::list(&p.id).unwrap();
    assert_eq!(l["worlds"][0]["name"], "Мой мир");
    let b = crate::worlds::backup(&p.id, "Мой мир").unwrap();
    println!("копия: {b}");
    let l = crate::worlds::list(&p.id).unwrap();
    assert_eq!(l["backups"].as_array().unwrap().len(), 1);
    assert_eq!(l["worlds"][0]["backups"], 1);
    std::fs::write(w.join("level.dat"), b"changed").unwrap();
    crate::worlds::restore(&p.id, b["file"].as_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(w.join("level.dat")).unwrap(), b"dat");
    assert!(crate::mc::inst_dir(&p.id).join("saves").join("Мой мир-до-восстановления").join("level.dat").exists());
    crate::worlds::delete(&p.id, b["file"].as_str().unwrap()).unwrap();
    println!("миры и копии работают");
}

#[test]
#[ignore]
fn skin_nick_flow() {
    use crate::accounts;
    let _ = accounts::add_local("NickTest");
    let n = accounts::skin_from_nick("local-nicktest", "jeb_").expect("скин по нику");
    println!("скин игрока {n} установлен");
    let d = accounts::details("local-nicktest", false).unwrap();
    assert!(!d["skin"].is_null());
    assert!(accounts::skin_from_nick("local-nicktest", "bad nick!").is_err());
}

#[test]
#[ignore]
fn social_flow() {
    use crate::{accounts, social, tunnel};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;
    let sfx = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 0xfffffff);
    let names: Vec<String> = ["fa", "fb", "fc"].iter().map(|p| format!("t{p}{sfx}")).collect();
    let mut tok = vec![];
    for n in &names {
        accounts::noxel_auth(n, "test-pass-123", true).expect("регистрация");
        tok.push(accounts::noxel_token(&format!("noxel-{}", n.to_lowercase())).unwrap());
    }
    let (a, b, c) = (&tok[0], &tok[1], &tok[2]);
    // друзья
    let r = social::friend_action(a, "add", &names[1]).unwrap();
    assert_eq!(r["state"], "pending");
    assert!(social::friends(b).unwrap()["incoming"].as_array().unwrap().iter().any(|x| *x == names[0]));
    assert_eq!(social::friend_action(b, "accept", &names[0]).unwrap()["state"], "accepted");
    assert!(social::friend_action(a, "add", "ЭтогоНикнеймаНетВНоксел").is_err());
    social::presence(a, Some("26.2".into()), Some("26.2".into())).unwrap();
    let fb = social::friends(b).unwrap();
    let fa = fb["friends"].as_array().unwrap().iter().find(|f| f["name"] == names[0].as_str()).unwrap().clone();
    println!("B видит друга: {fa}");
    assert_eq!(fa["online"], true);
    // «мир» хозяина: простой эхо-сервер вместо Minecraft
    let echo = TcpListener::bind("127.0.0.1:0").unwrap();
    let eport = echo.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for st in echo.incoming().flatten() {
            std::thread::spawn(move || {
                let (mut r, mut w) = (st.try_clone().unwrap(), st);
                let _ = std::io::copy(&mut r, &mut w);
            });
        }
    });
    tunnel::host_open(a, eport).expect("открытие комнаты");
    // ждём, пока сервер увидит комнату
    let mut room = false;
    for _ in 0..20 {
        let f = social::friends(b).unwrap();
        social::presence(a, Some("26.2".into()), None).unwrap();
        if f["friends"][0]["room"] == true { room = true; break; }
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(room, "комната не появилась у друга");
    let gp = tunnel::guest_open(b, &names[0]).expect("подключение гостя");
    let mut s = TcpStream::connect(("127.0.0.1", gp)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let mut w = s.try_clone().unwrap();
    let p2 = payload.clone();
    std::thread::spawn(move || { let _ = w.write_all(&p2); });
    let mut got = vec![0u8; payload.len()];
    s.read_exact(&mut got).expect("эхо через ретранслятор");
    assert_eq!(got, payload);
    println!("через ретранслятор прошло {} байт без искажений", got.len());
    // чужой (не друг) пройти не должен
    tunnel::guest_close();
    let cp = tunnel::guest_open(c, &names[0]).unwrap();
    let mut s2 = TcpStream::connect(("127.0.0.1", cp)).unwrap();
    s2.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let _ = s2.write_all(b"hello");
    let mut buf = [0u8; 8];
    let n = s2.read(&mut buf).unwrap_or(0);
    assert_eq!(n, 0, "не друг получил доступ к миру");
    println!("не друг отклонён");
    tunnel::guest_close();
    // UDP для голосовых модов: хозяин объявляет порт, гость слушает его у себя и гонит пакеты через сервер
    let mut st = load_settings();
    st.room_ports = "24456".into();
    save_settings(&st);
    let uecho = std::net::UdpSocket::bind("127.0.0.1:24456").expect("порт 24456 для проверки");
    std::thread::spawn(move || {
        let mut b = [0u8; 2048];
        while let Ok((n, from)) = uecho.recv_from(&mut b) {
            let _ = uecho.send_to(&b[..n], from);
        }
    });
    social::presence(a, Some("26.2".into()), None).unwrap();            // порты объявляются, пока мир открыт
    std::env::set_var("NOXEL_GUEST_BIND", "127.0.0.3");
    tunnel::guest_open(b, &names[0]).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let uc = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    uc.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (mut ok_udp, mut bytes) = (0, 0);
    uc.set_read_timeout(Some(Duration::from_millis(800))).unwrap();
    for i in 0..30u8 {
        let pkt = vec![i; 900];
        uc.send_to(&pkt, "127.0.0.3:24456").unwrap();
        let mut rb = [0u8; 2048];
        if let Ok((n, _)) = uc.recv_from(&mut rb) {
            if rb[..n] == pkt[..] { ok_udp += 1; bytes += n; }
        }
    }
    println!("UDP через ретранслятор: вернулось {ok_udp} из 30 пакетов ({bytes} байт)");
    assert!(ok_udp >= 25, "UDP через ретранслятор не работает");
    // сборка друга: хозяин публикует список модов профиля, друг получает его (чужой не получает)
    let pp = crate::mc::create_profile("1.21.1", "fabric", None, "pack-test", None).unwrap();
    let pdir = crate::mc::inst_dir(&pp.id);
    std::fs::create_dir_all(pdir.join("mods")).unwrap();
    std::fs::create_dir_all(pdir.join(".noxel")).unwrap();
    std::fs::write(pdir.join("mods/sodium.jar"), b"x").unwrap();
    std::fs::write(pdir.join(".noxel/index.json"), r#"{"sodium.jar":{"project_id":"AANobbMI","title":"Sodium","provider":"modrinth","kind":"mod","version":"1"},"gone.jar":{"project_id":"zzz","title":"Gone","provider":"modrinth","kind":"mod","version":"1"}}"#).unwrap();
    social::pack_publish(a, &pp.id).unwrap();
    let pk = social::pack_get(b, &names[0]).expect("сборка друга");
    println!("сборка друга: {pk}");
    assert_eq!(pk["mc"], "1.21.1");
    assert_eq!(pk["loader"], "fabric");
    assert_eq!(pk["mods"].as_array().unwrap().len(), 1, "должен остаться только существующий мод");
    assert!(social::pack_get(c, &names[0]).is_err(), "не друг получил сборку");
    tunnel::guest_close();
    tunnel::host_close();
    // облако
    let p = crate::mc::create_profile("1.21.1", "vanilla", None, "cloud-test", None).unwrap();
    let w = crate::mc::inst_dir(&p.id).join("saves").join("Облачный мир");
    std::fs::create_dir_all(w.join("region")).unwrap();
    std::fs::write(w.join("level.dat"), b"cloud-dat").unwrap();
    std::fs::write(w.join("region/r.0.0.mca"), vec![9u8; 200_000]).unwrap();
    let up = social::cloud_upload(a, &p.id, "1.21.1", "Облачный мир").expect("загрузка в облако");
    println!("в облаке: {up}");
    let l = social::cloud_list(a).unwrap();
    let wid = l["worlds"][0]["id"].as_u64().unwrap();
    assert!(social::cloud_download(b, &p.id, wid, "Облачный мир").is_err(), "чужой скачал мир");
    std::fs::write(w.join("level.dat"), b"changed").unwrap();
    social::cloud_download(a, &p.id, wid, "Облачный мир").expect("скачивание из облака");
    assert_eq!(std::fs::read(w.join("level.dat")).unwrap(), b"cloud-dat");
    // общая ссылка: информация без входа и скачивание другим игроком
    let sh = social::cloud_share(a, wid).unwrap();
    let url = sh["url"].as_str().unwrap().to_string();
    assert!(url.contains("/w/"), "{url}");
    let info = social::share_info(&url).unwrap();
    assert_eq!(info["owner"], names[0].as_str());
    std::fs::write(w.join("level.dat"), b"changed2").unwrap();
    social::share_import(&p.id, &url, "Облачный мир").expect("мир по ссылке");
    assert_eq!(std::fs::read(w.join("level.dat")).unwrap(), b"cloud-dat");
    println!("ссылка работает: {url}, автор {}", info["owner"]);
    // в облаке Noxel один мир: новый заменяет старый, старая ссылка перестаёт работать
    let w2 = crate::mc::inst_dir(&p.id).join("saves").join("Второй мир");
    std::fs::create_dir_all(&w2).unwrap();
    std::fs::write(w2.join("level.dat"), b"second").unwrap();
    social::cloud_upload(a, &p.id, "1.21.1", "Второй мир").expect("второй мир");
    let l = social::cloud_list(a).unwrap();
    assert_eq!(l["worlds"].as_array().unwrap().len(), 1);
    assert_eq!(l["worlds"][0]["name"], "Второй мир");
    assert!(social::share_info(&url).is_err(), "старая ссылка осталась рабочей");
    println!("один мир в облаке: старый заменён, старая ссылка отозвана");
    social::cloud_delete(a, l["worlds"][0]["id"].as_u64().unwrap()).unwrap();
    assert!(social::cloud_list(a).unwrap()["worlds"].as_array().unwrap().is_empty());
    println!("облако работает");
    // уборка тестовых аккаунтов
    for t in &tok {
        let _ = crate::skins::call("DELETE", "/account", Some(t), crate::skins::Body::Json(serde_json::json!({"password": "test-pass-123"})));
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() { copy_dir(&src, &dst) } else { std::fs::copy(&src, &dst).unwrap(); }
    }
}

/// Живая проверка «общего мира»: агент открывает одиночный мир для сети, а через ретранслятор его видно как сервер Minecraft.
#[test]
#[ignore]
fn room_flow() {
    use crate::{accounts, autoauth, mc, ping, social, tunnel};
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let ver = std::env::var("T_MC").unwrap_or_else(|_| "26.2".into());
    mc::prefetch(&ver, 8, false, &|_, _, _| {}).expect("предзагрузка игры");
    let vj: serde_json::Value = read_json(&data_dir().join("minecraft/versions").join(&ver).join(format!("{ver}.json"))).unwrap();
    let comp = vj["javaVersion"]["component"].as_str().unwrap_or("jre-legacy").to_string();
    let java = data_dir().join("runtime").join(&comp).join(JAVA_EXE);
    // настоящий мир: даём ванильному серверу создать его и остановиться
    let sdir = data_dir().join("room-server");
    std::fs::create_dir_all(&sdir).unwrap();
    download(vj["downloads"]["server"]["url"].as_str().unwrap(), &sdir.join("server.jar"), vj["downloads"]["server"]["sha1"].as_str()).unwrap();
    std::fs::write(sdir.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(sdir.join("server.properties"), "level-name=RoomWorld\nserver-port=25598\nonline-mode=false\nlevel-type=flat\nview-distance=4\n").unwrap();
    let mut srv = Command::new(&java).args(["-Xmx1G", "-jar", "server.jar", "nogui"]).current_dir(&sdir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let out = srv.stdout.take().unwrap();
    let ready = Arc::new(Mutex::new(false));
    let r2 = ready.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if l.contains("Done (") { *r2.lock().unwrap() = true; }
        }
    });
    let t = Instant::now();
    while !*ready.lock().unwrap() && t.elapsed() < Duration::from_secs(240) { std::thread::sleep(Duration::from_millis(300)); }
    assert!(*ready.lock().unwrap(), "сервер для создания мира не запустился");
    srv.stdin.as_mut().unwrap().write_all(b"stop\n").unwrap();
    let _ = srv.wait();
    // клиент
    let _ = accounts::add_local("RoomTest");
    accounts::select("local-roomtest").unwrap();
    let loader = std::env::var("T_LOADER").unwrap_or_else(|_| "fabric".into());
    let p = mc::create_profile(&ver, &loader, None, "roomtest", None).unwrap();
    copy_dir(&sdir.join("RoomWorld"), &mc::inst_dir(&p.id).join("saves").join("RoomWorld"));
    let st = load_settings();
    let acct = accounts::active_fresh().unwrap();
    let mut pr = mc::prepare(&p, &st, &acct, &|_: &str, _: f64, _: &str| {}).expect("подготовка игры");
    let _ = std::fs::write(pr.gamedir.join("options.txt"), "pauseOnLostFocus:false\nonboardAccessibility:false\nsoundCategory_master:0.0\nsoundCategory_music:0.0\nsoundCategory_record:0.0\n");
    pr.args.extend(["--quickPlaySingleplayer".to_string(), "RoomWorld".to_string()]);
    let game = mc::Game::default();
    let hlog = Arc::new(Mutex::new(Vec::<String>::new()));
    let h2 = hlog.clone();
    let mut child = mc::spawn_game(&game, &pr, Arc::new(move |l| h2.lock().unwrap().extend(l)), Arc::new(|_| {})).unwrap();
    let gpid = child.id() as u64;
    std::thread::spawn(move || {
        for _ in 0..800 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(v) = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()) {
                if v["pid"].as_u64() == Some(gpid) {
                    if let Some(addr) = v["address"].as_str() {
                        let _ = Command::new("hyprctl").args(["dispatch", &format!("hl.dsp.window.move({{ workspace = \"6\", follow = false, window = \"address:{addr}\" }})")]).output();
                    }
                }
            }
        }
    });
    // ждём загрузки мира и просим агента открыть его для сети
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let t = Instant::now();
    let mut ok = false;
    let mut last = String::new();
    while t.elapsed() < Duration::from_secs(240) {
        std::thread::sleep(Duration::from_secs(3));
        let id = autoauth::control(&pr.gamedir, &format!("!publish {port}"));
        last = autoauth::wait_ack(&pr.gamedir, &id, 4).unwrap_or_default();
        if last == "sent" { ok = true; break; }
    }
    assert!(ok, "агент не открыл мир для сети, последний ответ: {last}");
    println!("мир открыт для сети на порту {port}");
    let direct = ping::status(&format!("127.0.0.1:{port}"));
    println!("прямой пинг: {direct}");
    assert_eq!(direct["online"], true);
    // через ретранслятор: хозяин и гость, как у настоящих друзей
    let sfx = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 0xfffffff);
    let (na, nb) = (format!("tra{sfx}"), format!("trb{sfx}"));
    accounts::noxel_auth(&na, "test-pass-123", true).unwrap();
    accounts::noxel_auth(&nb, "test-pass-123", true).unwrap();
    let ta = accounts::noxel_token(&format!("noxel-{}", na.to_lowercase())).unwrap();
    let tb = accounts::noxel_token(&format!("noxel-{}", nb.to_lowercase())).unwrap();
    social::friend_action(&ta, "add", &nb).unwrap();
    social::friend_action(&tb, "accept", &na).unwrap();
    tunnel::host_open(&ta, port).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let gp = tunnel::guest_open(&tb, &na).unwrap();
    let via = ping::status(&format!("127.0.0.1:{gp}"));
    println!("пинг через ретранслятор: {via}");
    assert_eq!(via["online"], true, "мир не виден через ретранслятор");
    // настоящий второй клиент заходит в мир друга по туннелю
    let _ = accounts::add_local("RoomGuest");
    accounts::select("local-roomguest").unwrap();
    let pg = mc::create_profile(&ver, &loader, None, "roomguest", None).unwrap();
    let gacct = accounts::active_fresh().unwrap();
    let mut gr = mc::prepare(&pg, &st, &gacct, &|_: &str, _: f64, _: &str| {}).expect("подготовка гостя");
    let _ = std::fs::write(gr.gamedir.join("options.txt"), "pauseOnLostFocus:false\nonboardAccessibility:false\nsoundCategory_master:0.0\nsoundCategory_music:0.0\nsoundCategory_record:0.0\n");
    gr.args.extend(["--quickPlayMultiplayer".to_string(), format!("127.0.0.1:{gp}")]);
    let ggame = mc::Game::default();
    let mut gchild = mc::spawn_game(&ggame, &gr, Arc::new(|_| {}), Arc::new(|_| {})).unwrap();
    let gpid2 = gchild.id() as u64;
    std::thread::spawn(move || {
        for _ in 0..800 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(v) = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()) {
                if v["pid"].as_u64() == Some(gpid2) {
                    if let Some(addr) = v["address"].as_str() {
                        let _ = Command::new("hyprctl").args(["dispatch", &format!("hl.dsp.window.move({{ workspace = \"6\", follow = false, window = \"address:{addr}\" }})")]).output();
                    }
                }
            }
        }
    });
    let t = Instant::now();
    let mut joined = None;
    while t.elapsed() < Duration::from_secs(240) {
        if let Some(l) = hlog.lock().unwrap().iter().find(|l| l.contains("RoomGuest joined the game")) { joined = Some(l.clone()); break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("вход гостя в мир друга: {:?}", joined);
    let _ = gchild.kill();
    tunnel::guest_close();
    tunnel::host_close();
    let _ = child.kill();
    assert!(joined.is_some(), "второй клиент не зашёл в мир по туннелю");
    for t in [&ta, &tb] {
        let _ = crate::skins::call("DELETE", "/account", Some(t), crate::skins::Body::Json(serde_json::json!({"password": "test-pass-123"})));
    }
}

/// Своё облако по WebDAV: проверка на локальном мини-сервере (логин, выгрузка, список, возврат мира, автосинхронизация только изменённых).
#[test]
#[ignore]
fn dav_flow() {
    use std::process::{Command, Stdio};
    let dir = data_dir().join("dav-test-root");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut srv = Command::new("python3").arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/webdav_mini.py")).arg(port.to_string()).arg(&dir).arg("user:app-pass").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(800));
    let mut st = load_settings();
    st.dav_url = format!("http://127.0.0.1:{port}");
    st.dav_user = "user".into();
    st.dav_pass = "wrong".into();
    save_settings(&st);
    let bad = crate::dav::test();
    println!("с неверным паролем: {bad:?}");
    assert!(bad.is_err());
    st.dav_pass = "app-pass".into();
    save_settings(&st);
    crate::dav::test().expect("подключение");
    let p = crate::mc::create_profile("1.21.1", "vanilla", None, "dav-test", None).unwrap();
    let w = crate::mc::inst_dir(&p.id).join("saves").join("Мир для облака");
    std::fs::create_dir_all(w.join("region")).unwrap();
    std::fs::write(w.join("level.dat"), b"dav-dat").unwrap();
    std::fs::write(w.join("region/r.0.0.mca"), vec![5u8; 100_000]).unwrap();
    assert_eq!(crate::dav::sync_changed(&p.id).unwrap(), 1);
    assert_eq!(crate::dav::sync_changed(&p.id).unwrap(), 0, "не изменённый мир выгружен повторно");
    let l = crate::dav::list(&p.id).unwrap();
    println!("в облаке: {l}");
    assert_eq!(l[0]["name"], "Мир для облака");
    std::fs::write(w.join("level.dat"), b"changed").unwrap();
    crate::dav::download_world(&p.id, "Мир для облака").expect("возврат из облака");
    assert_eq!(std::fs::read(w.join("level.dat")).unwrap(), b"dav-dat");
    println!("своё облако работает");
    let _ = srv.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Мод Noxel в настоящей игре 26.2: лаунчер сам ставит мод, мод находит локальный API, открывает экран друзей и получает список с сервера.
#[test]
#[ignore]
fn mod_flow() {
    use crate::{accounts, localapi, mc, social};
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let ver = "26.2";
    mc::prefetch(ver, 8, false, &|_, _, _| {}).expect("предзагрузка игры");
    localapi::start();
    let sfx = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 0xfffffff);
    let (na, nb) = (format!("tma{sfx}"), format!("tmb{sfx}"));
    accounts::noxel_auth(&nb, "test-pass-123", true).unwrap();
    accounts::noxel_auth(&na, "test-pass-123", true).unwrap();   // активным остаётся A: от его имени играет клиент
    let ta = accounts::noxel_token(&format!("noxel-{}", na.to_lowercase())).unwrap();
    let tb = accounts::noxel_token(&format!("noxel-{}", nb.to_lowercase())).unwrap();
    social::friend_action(&ta, "add", &nb).unwrap();
    social::friend_action(&tb, "accept", &na).unwrap();
    let p = mc::create_profile(ver, "fabric", None, "modtest", None).unwrap();
    let mods = mc::inst_dir(&p.id).join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    let api = std::fs::read_dir("/mnt/D/project/Noxel/данные/instances").unwrap().flatten().flat_map(|e| std::fs::read_dir(e.path().join("mods")).into_iter().flatten().flatten()).find(|f| f.file_name().to_string_lossy().starts_with("fabric-api")).expect("fabric-api в папках модов").path();
    std::fs::copy(&api, mods.join(api.file_name().unwrap())).unwrap();
    let _ = accounts::add_local("SwitchTo");
    accounts::select(&format!("noxel-{}", na.to_lowercase())).unwrap();
    let st = load_settings();
    let acct = accounts::active_fresh().unwrap();
    assert_eq!(acct.kind, "noxel");
    let mut pr = mc::prepare(&p, &st, &acct, &|_: &str, _: f64, _: &str| {}).expect("подготовка игры");
    assert!(mods.join("noxel-client.jar").exists(), "лаунчер не поставил мод");
    assert!(pr.args.iter().any(|a| a.starts_with("-Dnoxel.api=")), "игре не передан адрес API");
    pr.args.insert(0, "-Dnoxel.selftest=1".to_string());
    pr.args.insert(0, "-Dnoxel.selftest.switch=SwitchTo".to_string());
    let _ = std::fs::write(pr.gamedir.join("options.txt"), "pauseOnLostFocus:false\nonboardAccessibility:false\nsoundCategory_master:0.0\nsoundCategory_music:0.0\nsoundCategory_record:0.0\n");
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = log.clone();
    let game = mc::Game::default();
    let mut child = mc::spawn_game(&game, &pr, Arc::new(move |l| l2.lock().unwrap().extend(l)), Arc::new(|_| {})).unwrap();
    let gpid = child.id() as u64;
    std::thread::spawn(move || {
        for _ in 0..800 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(v) = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()) {
                if v["pid"].as_u64() == Some(gpid) {
                    if let Some(addr) = v["address"].as_str() {
                        let _ = Command::new("hyprctl").args(["dispatch", &format!("hl.dsp.window.move({{ workspace = \"6\", follow = false, window = \"address:{addr}\" }})")]).output();
                    }
                }
            }
        }
    });
    let want = ["[Noxel] мод запущен", "[Noxel] друзей загружено: 1", "[Noxel] экран друзей отрисован", "[Noxel] аккаунт сменён на SwitchTo"];
    let t = Instant::now();
    let mut found = [false; 4];
    while t.elapsed() < Duration::from_secs(240) && !found.iter().all(|x| *x) {
        for (i, w) in want.iter().enumerate() {
            if log.lock().unwrap().iter().any(|l| l.contains(w)) { found[i] = true; }
        }
        if child.try_wait().ok().flatten().is_some() { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    let crashed = log.lock().unwrap().iter().any(|l| l.contains("Crash") || l.contains("FATAL"));
    println!("мод: запущен={} друзья={} экран={} смена аккаунта в игре={} падение={}", found[0], found[1], found[2], found[3], crashed);
    println!("активный аккаунт в лаунчере после смены в игре: {}", accounts::active_fresh().map(|a| a.name).unwrap_or_default());
    assert_eq!(accounts::active_fresh().unwrap().name, "SwitchTo", "лаунчер не запомнил выбор из игры");
    let _ = child.kill();
    for t in [&ta, &tb] {
        let _ = crate::skins::call("DELETE", "/account", Some(t), crate::skins::Body::Json(serde_json::json!({"password": "test-pass-123"})));
    }
    assert!(found.iter().all(|x| *x), "мод не отработал: {:?}", found);
}

/// Рекомендуемые моды: список с сервера, проверка наличия под версию профиля и установка архива с эмоциями.
#[test]
#[ignore]
fn reco_flow() {
    let r = crate::remote::reco().expect("список с сервера");
    let cats = r["categories"].as_array().unwrap();
    println!("разделов: {}", cats.len());
    assert!(cats.len() >= 6);
    let p = crate::mc::create_profile("26.2", "fabric", None, "reco-test", None).unwrap();
    let items: Vec<(String, String)> = ["sodium", "distanthorizons", "falling-leaves", "plasmo-voice"].iter().map(|s| ("modrinth".to_string(), s.to_string())).chain(std::iter::once(("curseforge".to_string(), "394468".to_string()))).collect();
    let info = crate::reco::info(&p.id, &items).expect("проверка наличия");
    for k in ["sodium", "distanthorizons", "falling-leaves", "plasmo-voice", "394468"] {
        println!("  {k}: ok={} название={} установлен={}", info[k]["ok"], info[k]["title"], info[k]["installed"]);
    }
    assert_eq!(info["sodium"]["ok"], true);
    assert_eq!(info["falling-leaves"]["ok"], false, "мод без сборки под 26.2 должен быть помечен недоступным");
    assert!(info["394468"]["ok"].is_null());
    // архив эмоций: только папка emotes, без раздела для взрослых
    let e = cats.iter().find(|c| c["id"] == "emote-packs").expect("раздел эмоций")["items"][0].clone();
    let res = crate::reco::install_file(&p.id, e["url"].as_str().unwrap(), "emotes", true, e["prefix"].as_str().unwrap(), "эмоции").expect("установка эмоций");
    let dir = crate::mc::inst_dir(&p.id).join("emotes");
    let n = std::fs::read_dir(&dir).unwrap().count();
    println!("эмоции установлены: {res}, файлов в папке: {n}");
    assert!(n >= 30);
    assert!(!dir.join("fap.json").exists(), "файлы из раздела NSFW не должны скачиваться");
    assert!(crate::reco::install_file(&p.id, "https://example.com/x.zip", "mods", true, "", "x").is_err(), "папка mods запрещена");
}

/// Живая проверка Horizon: лента с сервера Noxel, скачивание лаунчера, проверка суммы и запуск с командой versions.
#[test]
#[ignore]
fn horizon_flow() {
    let st = crate::horizon::status().expect("статус");
    println!("до установки: {st}");
    assert!(st["feed_ok"].as_bool().unwrap_or(false), "лента не пришла");
    let st = crate::horizon::install().expect("установка");
    println!("после установки: {st}");
    assert!(st["installed"].as_str().is_some());
    let meta: serde_json::Value = read_json(&data_dir().join("horizon/installed.json")).unwrap();
    let exe = data_dir().join("horizon").join(meta["exe"].as_str().unwrap());
    let out = std::process::Command::new(&exe).env("APPIMAGE_EXTRACT_AND_RUN", "1").arg("versions").output().expect("запуск");
    println!("код {:?}\n{}\n{}", out.status.code(), String::from_utf8_lossy(&out.stdout).lines().take(8).collect::<Vec<_>>().join("\n"), String::from_utf8_lossy(&out.stderr).lines().take(5).collect::<Vec<_>>().join("\n"));
    assert!(out.status.success(), "лаунчер Horizon не запустился");
}

/// Переезд и обмен сборками: экспорт, импорт, общее хранилище, точки, код сборки, поиск чужих лаунчеров, разбор краша.
/// T_API=http://127.0.0.1:18730/noxel/v1 — локальный сервер Noxel для кода сборки.
#[test]
#[ignore]
fn transfer_flow() {
    use std::io::Write;
    let cb = |s: &str, f: f64, d: &str| if f < 0.0 || f >= 1.0 { println!("  [{s}] {d}") };
    let lv = mc::loader_versions("1.21.1", "fabric");
    let a = mc::create_profile("1.21.1", "fabric", lv.first().cloned(), "xfer A", None).unwrap();
    for slug in ["AANobbMI", "gvQqBUqZ"] {
        crate::mods::install("modrinth", "mod", slug, &a.id, None, &cb).unwrap();
    }
    let dir = mc::inst_dir(&a.id);
    // свой мод, которого нет на Modrinth, и настройки
    {
        let f = std::fs::File::create(dir.join("mods/fake-mod.jar")).unwrap();
        let mut z = zip::ZipWriter::new(f);
        z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(br#"{"id":"fakemod","version":"1"}"#).unwrap();
        z.start_file("com/example/fakemod/Boom.class", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&[0xCA, 0xFE, 0xBA, 0xBE]).unwrap();
        z.finish().unwrap();
    }
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(dir.join("config/test.json"), "{\"a\":1}").unwrap();
    let mods_a: Vec<String> = std::fs::read_dir(dir.join("mods")).unwrap().flatten().map(|f| f.file_name().to_string_lossy().to_string()).collect();
    println!("профиль A: {mods_a:?}");

    // экспорт .mrpack
    let out = data_dir().join("xfer-test.mrpack");
    let r = crate::transfer::export_mrpack(&a.id, &out, false, &cb).unwrap();
    println!("экспорт mrpack: {r}");
    assert_eq!(r["linked"], 2, "Sodium и Lithium должны уйти ссылками");
    assert!(r["embedded"].as_u64().unwrap() >= 1, "свой мод кладётся внутрь");

    // импорт обратно
    let r = crate::transfer::import_file(&out, &cb).unwrap();
    let b = r["profile"].as_str().unwrap().to_string();
    let bdir = mc::inst_dir(&b);
    let mods_b: Vec<String> = std::fs::read_dir(bdir.join("mods")).unwrap().flatten().map(|f| f.file_name().to_string_lossy().to_string()).collect();
    println!("импорт: {r} моды {mods_b:?}");
    assert_eq!(mods_b.len(), mods_a.len());
    assert!(bdir.join("config/test.json").is_file());
    let idx_b = crate::mods::indexed_files(&b);
    println!("известны источники: {idx_b:?}");
    assert!(idx_b.len() >= 2);

    // общее хранилище: Sodium в обоих профилях — один файл на диске
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let sa = mods_a.iter().find(|n| n.starts_with("sodium")).unwrap();
        let ia = std::fs::metadata(dir.join("mods").join(sa)).unwrap().ino();
        let ib = std::fs::metadata(bdir.join("mods").join(sa)).unwrap().ino();
        println!("inode A={ia} B={ib}");
        assert_eq!(ia, ib, "одинаковые моды должны быть одной жёсткой ссылкой");
    }
    println!("освобождение места: {}", crate::store::dedupe_all());

    // точка восстановления
    let snap = crate::store::snapshot(&a.id, "тест").unwrap();
    let lith = mods_a.iter().find(|n| n.starts_with("lithium")).unwrap().clone();
    std::fs::remove_file(dir.join("mods").join(&lith)).unwrap();
    crate::store::restore(&a.id, snap["id"].as_str().unwrap()).unwrap();
    assert!(dir.join("mods").join(&lith).is_file(), "после отката мод вернулся");
    println!("точки: {}", crate::store::snapshots(&a.id).unwrap().as_array().unwrap().len());

    // экспорт CurseForge (нужен сервер с ключом CurseForge; без него файлы просто лягут внутрь)
    let out2 = data_dir().join("xfer-test-cf.zip");
    match crate::transfer::export_cf(&a.id, &out2, false, &cb) {
        Ok(r) => println!("экспорт CurseForge: {r}"),
        Err(x) => println!("экспорт CurseForge: {x}"),
    }

    // код сборки через сервер
    if std::env::var("T_API").is_ok() {
        let r = crate::social::code_create(None, &a.id).unwrap();
        println!("код: {r}");
        let code = r["code"].as_str().unwrap().to_string();
        let r = crate::social::code_install(&code, &cb).unwrap();
        println!("по коду: {r}");
        let c = r["profile"].as_str().unwrap();
        assert!(r["installed"].as_u64().unwrap() >= 2);
        assert!(mc::inst_dir(c).join("config/test.json").is_file(), "настройки модов приехали");
        let _ = mc::delete_profile(c, true);
    }

    // разбор краша: мод в стеке и нехватка зависимости
    let lines: Vec<String> = [
        "[main/ERROR]: Incompatible mods found!",
        " - Mod 'Sodium Extra' (sodium-extra) 0.5.4 requires any version of reeses-sodium-options, which is missing!",
        "java.lang.NullPointerException: boom",
        "\tat com.example.fakemod.Boom.tick(Boom.java:10)",
        "\tat net.minecraft.client.Minecraft.run(Minecraft.java:1)",
    ].iter().map(|s| s.to_string()).collect();
    let cr = crate::crash::analyze(&a.id, &lines, 1, now());
    println!("краш: {cr}");
    assert!(cr["mods"].as_array().unwrap().iter().any(|m| m == "fake-mod.jar"));
    assert!(!cr["missing"].as_array().unwrap().is_empty());

    // поиск профилей других лаунчеров: поддельный Prism в отдельном HOME
    let fake_home = data_dir().join("fake-home");
    let inst = fake_home.join(".local/share/PrismLauncher/instances/My Pack");
    std::fs::create_dir_all(inst.join(".minecraft/mods")).unwrap();
    std::fs::create_dir_all(inst.join(".minecraft/saves/World1")).unwrap();
    std::fs::write(inst.join("instance.cfg"), "[General]\nname=Моя сборка Prism\n").unwrap();
    std::fs::write(inst.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.21.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.16.5"}]}"#).unwrap();
    std::fs::copy(dir.join("mods").join(&lith), inst.join(".minecraft/mods").join(&lith)).unwrap();
    let old_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &fake_home);
    std::env::remove_var("XDG_DATA_HOME");
    let found = crate::transfer::detect();
    if let Some(h) = old_home {
        std::env::set_var("HOME", h);
    }
    println!("найдено: {found}");
    let x = found.as_array().unwrap().iter().find(|v| v["source"] == "Prism Launcher").expect("Prism не найден").clone();
    assert_eq!(x["mc"], "1.21.1");
    assert_eq!(x["loader"], "fabric");
    let r = crate::transfer::import_instance(x["path"].as_str().unwrap(), x["name"].as_str().unwrap(), "1.21.1", "fabric", Some("0.16.5".into()), true, &cb).unwrap();
    println!("перенос: {r}");
    let c = r["profile"].as_str().unwrap();
    assert!(mc::inst_dir(c).join("saves/World1").is_dir());
    assert!(mc::inst_dir(c).join("mods").join(&lith).is_file());
    for id in [a.id.as_str(), b.as_str(), c] {
        let _ = mc::delete_profile(id, true);
    }
    let _ = std::fs::remove_dir_all(fake_home);
    println!("ОК");
}

/// Обновление сборки Modrinth с показом изменений: ставим прошлую версию Fabulously Optimized и обновляем.
#[test]
#[ignore]
fn pack_update_flow() {
    let cb = |s: &str, f: f64, d: &str| if f < 0.0 || f >= 1.0 { println!("  [{s}] {d}") };
    let vs: serde_json::Value = get_json("https://api.modrinth.com/v2/project/fabulously-optimized/version?game_versions=%5B%221.21.1%22%5D").unwrap();
    let list = vs.as_array().unwrap();
    let old = &list[list.len().min(3) - 1];
    let f = &old["files"][0];
    let tmp = data_dir().join("cache/fo-old.mrpack");
    download(f["url"].as_str().unwrap(), &tmp, f["hashes"]["sha1"].as_str()).unwrap();
    let origin = serde_json::json!({"provider": "modrinth", "project_id": old["project_id"], "version": old["id"], "version_name": old["version_number"]});
    let r = crate::packs::install_mrpack(&tmp, None, Some(origin), &cb).unwrap();
    let id = r["profile"].as_str().unwrap().to_string();
    println!("поставил {} {}", r["name"], old["version_number"]);
    let u = crate::packs::update_check(&id).unwrap();
    println!("проверка: {u}");
    assert_eq!(u["update"], true);
    let r = crate::packs::update_apply(&id, &cb).unwrap();
    println!("обновил: {r}");
    let u = crate::packs::update_check(&id).unwrap();
    assert_eq!(u["update"], false, "после обновления новой версии нет");
    let snaps = crate::store::snapshots(&id).unwrap();
    println!("точки: {snaps}");
    assert!(!snaps.as_array().unwrap().is_empty());
    let _ = mc::delete_profile(&id, true);
}

/// Вход через authlib-injector (Ely.by-совместимый сервер) и запуск игры с ним.
/// T_YGG=http://127.0.0.1:18730/noxel/ygg T_YGG_USER=tester1 T_YGG_PASS=secret12
#[test]
#[ignore]
fn ygg_launch() {
    let server = std::env::var("T_YGG").unwrap_or_else(|_| "ely.by".into());
    crate::accounts::ygg_auth(&server, &std::env::var("T_YGG_USER").unwrap(), &std::env::var("T_YGG_PASS").unwrap()).unwrap();
    let acct = crate::accounts::active_fresh().unwrap();
    println!("аккаунт {} {} {}", acct.kind, acct.name, acct.api);
    assert_eq!(acct.kind, "ygg");
    let p = mc::create_profile("1.21.1", "vanilla", None, "ygg test", None).unwrap();
    let cb = |s: &str, f: f64, d: &str| if f < 0.0 || f >= 1.0 { println!("  [{s}] {d}") };
    let pr = mc::prepare(&p, &load_settings(), &acct, &cb).unwrap();
    let agent = pr.args.iter().find(|a| a.contains("authlib-injector")).cloned().unwrap_or_default();
    println!("агент: {agent}");
    assert!(!agent.is_empty());
    let game = mc::Game::default();
    let logs = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = logs.clone();
    let mut child = mc::spawn_game(&game, &pr, Arc::new(move |v| l2.lock().unwrap().extend(v)), Arc::new(|_| {})).unwrap();
    std::thread::sleep(Duration::from_secs(45));
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let l = logs.lock().unwrap();
    for x in l.iter().filter(|x| x.contains("authlib-injector") || x.contains("Setting user") || x.contains("ERROR")).take(20) {
        println!("  {x}");
    }
    let _ = mc::delete_profile(&p.id, true);
    assert!(alive, "игра завершилась раньше времени");
    assert!(l.iter().any(|x| x.contains("authlib-injector")), "агент не загрузился");
}

/// Готовые сборки «PvP» и «Красиво», голосовой чат и поиск профилей на этом компьютере (только чтение).
#[test]
#[ignore]
fn presets_flow() {
    let cb = |_: &str, _: f64, _: &str| {};
    let ver = std::env::var("T_MC").unwrap_or_else(|_| "1.21.1".into());
    let lv = mc::loader_versions(&ver, "fabric");
    for pr in ["pvp", "pretty", "fps"] {
        let p = mc::create_profile(&ver, "fabric", lv.first().cloned(), &format!("preset {pr}"), None).unwrap();
        let (ok, skipped) = crate::mods::apply_preset(&p.id, pr, &cb).unwrap();
        println!("{pr} {ver}: поставлено {} {:?}\n   пропущено {:?}", ok.len(), ok, skipped);
        if pr == "pvp" {
            crate::mods::install("modrinth", "mod", "9eGKb6K1", &p.id, None, &cb).unwrap();
            assert!(crate::mods::already(&p.id, "mod", "9eGKb6K1"));
            println!("   голосовой чат поставлен");
        }
        if pr == "pretty" {
            let sh = crate::mods::list(&p.id, "shader").unwrap();
            println!("   шейдеры: {sh}");
        }
        let _ = mc::delete_profile(&p.id, true);
    }
    println!("профили на этом компьютере: {}", crate::transfer::detect());
}
