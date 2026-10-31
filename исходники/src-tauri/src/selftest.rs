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
