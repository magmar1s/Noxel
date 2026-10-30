#!/usr/bin/env python3
"""Noxel API: аккаунты с паролем, скины и плащи с историей, синхронизация данных.

Только стандартная библиотека Python 3. Слушает 127.0.0.1, наружу его выставляет nginx по HTTPS.
Пароли хранятся как scrypt-хеши, токены как sha256. Картинки лежат по хешу содержимого,
nginx отдаёт их напрямую по адресу {BASE}/f/<хеш>.png: этот адрес можно давать
SkinsRestorer командой /skin url.
"""
import hashlib, hmac, json, os, re, secrets, sqlite3, struct, threading, time, urllib.error, urllib.request, zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

DATA = os.environ.get("NOXEL_DATA", "/var/lib/noxel")
FILES = os.path.join(DATA, "files")
BASE = os.environ.get("NOXEL_BASE", "https://mr1s.site/noxel").rstrip("/")
HOST = os.environ.get("NOXEL_HOST", "127.0.0.1")
PORT = int(os.environ.get("NOXEL_PORT", "8730"))
NAME_RE = re.compile(r"^[A-Za-z0-9_]{3,16}$")
MAX_BODY = 300 * 1024
MAX_PNG = 100 * 1024
MAX_DATA = 512 * 1024
CF_KEY_FILE = os.environ.get("NOXEL_CF_KEY_FILE", "/etc/noxel/cf.key")
ADMIN_FILE = os.environ.get("NOXEL_ADMIN_FILE", "/etc/noxel/admin.token")
SERVERS = os.path.join(DATA, "servers.json")
LAUNCHER = os.path.join(DATA, "launcher.json")
NEWS = os.path.join(DATA, "news.json")
# через прокси разрешены только безопасные запросы на чтение
CF_ALLOWED = re.compile(r"^(mods/search|mods/\d+|mods/\d+/description|mods/\d+/files|mods/\d+/files/\d+|mods/\d+/files/\d+/download-url|mods/\d+/files/\d+/changelog)$")
_cf_cache = {}
SKIN_SIZES = {(64, 64), (64, 32)}
CAPE_SIZES = {(64, 32), (22, 17), (46, 22)}

os.makedirs(FILES, exist_ok=True)
_local = threading.local()
_hits = {}
_hits_lock = threading.Lock()


def db():
    c = getattr(_local, "c", None)
    if c is None:
        c = _local.c = sqlite3.connect(os.path.join(DATA, "noxel.db"), timeout=15)
        c.row_factory = sqlite3.Row
        c.execute("PRAGMA journal_mode=WAL")
        c.execute("PRAGMA foreign_keys=ON")
    return c


def init():
    c = db()
    c.executescript("""
    CREATE TABLE IF NOT EXISTS accounts(
      id INTEGER PRIMARY KEY, name TEXT UNIQUE COLLATE NOCASE NOT NULL, uuid TEXT UNIQUE NOT NULL,
      salt BLOB NOT NULL, pw BLOB NOT NULL, created INTEGER NOT NULL,
      skin_id INTEGER, cape_id INTEGER, data TEXT NOT NULL DEFAULT '{}');
    CREATE TABLE IF NOT EXISTS tokens(
      hash TEXT PRIMARY KEY, account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      created INTEGER NOT NULL, used INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS files(
      id INTEGER PRIMARY KEY, account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      kind TEXT NOT NULL, hash TEXT NOT NULL, model TEXT NOT NULL DEFAULT 'classic', created INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS files_acc ON files(account, kind, created);
    """)
    c.commit()


def offline_uuid(name):
    b = bytearray(hashlib.md5(("OfflinePlayer:" + name).encode()).digest())
    b[6] = (b[6] & 0x0F) | 0x30
    b[8] = (b[8] & 0x3F) | 0x80
    return b.hex()


def hash_pw(pw, salt):
    return hashlib.scrypt(pw.encode(), salt=salt, n=2 ** 14, r=8, p=1, dklen=32)


def png_size(b, allowed):
    """Проверяет сигнатуру, контрольные суммы блоков и размеры. Возвращает (w, h)."""
    if len(b) > MAX_PNG or b[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("это не PNG или файл слишком большой")
    pos, size, idat, end = 8, None, False, False
    while pos + 12 <= len(b):
        (ln,) = struct.unpack(">I", b[pos:pos + 4])
        typ, data = b[pos + 4:pos + 8], b[pos + 8:pos + 8 + ln]
        if len(data) != ln or pos + 12 + ln > len(b):
            raise ValueError("повреждённый PNG")
        (crc,) = struct.unpack(">I", b[pos + 8 + ln:pos + 12 + ln])
        if zlib.crc32(typ + data) & 0xFFFFFFFF != crc:
            raise ValueError("повреждённый PNG")
        if typ == b"IHDR":
            size = struct.unpack(">II", data[:8])
        elif typ == b"IDAT":
            idat = True
        elif typ == b"IEND":
            end = True
            break
        pos += 12 + ln
    if not (size and idat and end):
        raise ValueError("повреждённый PNG")
    if size not in allowed:
        raise ValueError("неверный размер %dx%d, нужен один из: %s" % (size[0], size[1], ", ".join("%dx%d" % s for s in sorted(allowed))))
    return size


def read_secret(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return ""


def clean_news(v):
    """Проверяет объявления, присланные администратором: {"items": [{title, body, kind, date, link, pinned}]}."""
    if not isinstance(v, dict) or not isinstance(v.get("items"), list) or len(v["items"]) > 100:
        raise Err(400, "ожидается {\"items\": [...]}, не больше 100 записей")
    out = []
    for i, x in enumerate(v["items"]):
        if not isinstance(x, dict):
            raise Err(400, "запись %d: ожидается объект" % i)
        title, body = str(x.get("title", "")).strip()[:120], str(x.get("body", "")).strip()[:4000]
        if not title:
            raise Err(400, "запись %d: нужен заголовок" % i)
        link = str(x.get("link", "")).strip()[:300]
        if link and not link.startswith(("https://", "http://")):
            raise Err(400, "запись %d: ссылка должна начинаться с https://" % i)
        kind = x.get("kind") if x.get("kind") in ("info", "update", "important") else "info"
        date = x.get("date") if isinstance(x.get("date"), int) and 0 < x["date"] < 4102444800 else int(time.time())
        out.append({"id": str(x.get("id") or ("n%d-%d" % (date, i)))[:40], "title": title, "body": body, "kind": kind, "date": date, "link": link, "pinned": bool(x.get("pinned"))})
    return {"version": int(time.time()), "items": out}


def clean_servers(v):
    """Проверяет и нормализует список серверов, присланный администратором."""
    if not isinstance(v, dict) or not isinstance(v.get("servers"), list) or len(v["servers"]) > 200:
        raise Err(400, "ожидается {\"servers\": [...]}, не больше 200 записей")
    out = []
    for n, s in enumerate(v["servers"]):
        if not isinstance(s, dict):
            continue
        name = str(s.get("name", "")).strip()[:60]
        if not name:
            raise Err(400, "у сервера нет названия")
        mods = []
        for m in (s.get("mods") or [])[:60]:
            if not isinstance(m, dict) or m.get("provider") not in ("modrinth", "curseforge"):
                continue
            mid = str(m.get("id", "")).strip()[:80]
            if re.match(r"^[A-Za-z0-9_.\-]+$", mid):
                mods.append({"provider": m["provider"], "id": mid, "name": str(m.get("name") or mid)[:60], "required": bool(m.get("required"))})
        link = str(s.get("link", ""))[:200]
        out.append({
            "id": (re.sub(r"[^a-z0-9\-]+", "-", str(s.get("id") or name).lower()).strip("-") or "server-%d" % (n + 1))[:40],
            "name": name, "host": str(s.get("host", "")).strip()[:100], "desc": str(s.get("desc", ""))[:500],
            "tags": [str(t)[:24] for t in (s.get("tags") or [])[:8]], "mc": str(s.get("mc", ""))[:20], "loader": str(s.get("loader", ""))[:12],
            "link": link if link.startswith("https://") or link.startswith("http://") else "", "official": bool(s.get("official")), "mods": mods})
    now = int(time.time())
    return {"version": now, "updated": now, "servers": out}


def throttle(key, limit, per=60):
    now = time.time()
    with _hits_lock:
        arr = [t for t in _hits.get(key, []) if now - t < per]
        if len(arr) >= limit:
            _hits[key] = arr
            return False
        arr.append(now)
        _hits[key] = arr
        if len(_hits) > 5000:
            for k in list(_hits)[:2500]:
                _hits.pop(k, None)
    return True


class Err(Exception):
    def __init__(self, code, msg):
        self.code, self.msg = code, msg


def item(r, cur):
    return {"id": r["id"], "url": "%s/f/%s.png" % (BASE, r["hash"]), "hash": r["hash"], "model": r["model"], "created": r["created"], "current": r["id"] == cur}


def listing(acc, kind):
    rows = db().execute("SELECT * FROM files WHERE account=? AND kind=? ORDER BY created DESC, id DESC LIMIT 200", (acc["id"], kind)).fetchall()
    cur = acc["skin_id" if kind == "skin" else "cape_id"]
    items = [item(r, cur) for r in rows]
    return {"current": next((i for i in items if i["current"]), None), "history": items}


class H(BaseHTTPRequestHandler):
    server_version = "Noxel"
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    # ── вспомогательное ──
    def ip(self):
        return self.headers.get("X-Real-IP") or self.client_address[0]

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        if n > MAX_BODY and not self.path.startswith(("/noxel/v1/data", "/noxel/v1/servers")):
            raise Err(413, "слишком большой запрос")
        if n > MAX_DATA:
            raise Err(413, "слишком большой запрос")
        return self.rfile.read(n) if n else b""

    def jbody(self):
        try:
            v = json.loads(self.body() or b"{}")
            return v if isinstance(v, dict) else {}
        except ValueError:
            raise Err(400, "неверный JSON")

    def send(self, code, obj=None, raw=None, ctype="application/json"):
        data = raw if raw is not None else json.dumps(obj if obj is not None else {}, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype + ("; charset=utf-8" if ctype == "application/json" else ""))
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(data)

    def auth(self):
        t = (self.headers.get("Authorization") or "")[7:].strip() if (self.headers.get("Authorization") or "").startswith("Bearer ") else ""
        if not t:
            raise Err(401, "нужен вход")
        h = hashlib.sha256(t.encode()).hexdigest()
        r = db().execute("SELECT a.* FROM tokens t JOIN accounts a ON a.id=t.account WHERE t.hash=?", (h,)).fetchone()
        if not r:
            raise Err(401, "сессия недействительна, войди заново")
        db().execute("UPDATE tokens SET used=? WHERE hash=?", (int(time.time()), h))
        db().commit()
        return r

    def new_token(self, acc_id):
        t = secrets.token_urlsafe(32)
        c = db()
        c.execute("INSERT INTO tokens(hash,account,created,used) VALUES(?,?,?,?)", (hashlib.sha256(t.encode()).hexdigest(), acc_id, int(time.time()), int(time.time())))
        c.execute("DELETE FROM tokens WHERE account=? AND hash NOT IN (SELECT hash FROM tokens WHERE account=? ORDER BY used DESC LIMIT 10)", (acc_id, acc_id))
        c.commit()
        return t

    def view(self, a):
        cur_s = db().execute("SELECT * FROM files WHERE id=?", (a["skin_id"],)).fetchone() if a["skin_id"] else None
        cur_c = db().execute("SELECT * FROM files WHERE id=?", (a["cape_id"],)).fetchone() if a["cape_id"] else None
        return {"name": a["name"], "uuid": a["uuid"], "created": a["created"],
                "skin": item(cur_s, cur_s["id"]) if cur_s else None, "cape": item(cur_c, cur_c["id"]) if cur_c else None}

    def cf_proxy(self, path, query):
        """Запросы к CurseForge с ключом сервера: в лаунчере ключа нет, игроки его не увидят."""
        key = read_secret(CF_KEY_FILE)
        if not key:
            raise Err(503, "CurseForge на сервере не настроен")
        if not CF_ALLOWED.match(path):
            raise Err(404, "не найдено")
        if len(query) > 700 or not re.match(r"^[\w=&%.,\-+ ]*$", query):
            raise Err(400, "неверный запрос")
        if not throttle(("cf", self.ip()), 90):
            raise Err(429, "слишком много запросов, подожди минуту")
        ck = path + "?" + query
        hit = _cf_cache.get(ck)
        if hit and time.time() - hit[0] < 300:
            return self.send(200, raw=hit[1])
        url = "https://api.curseforge.com/v1/" + path + ("?" + query if query else "")
        code, body = 0, b""
        # CurseForge иногда отвечает 403 «пачками» с адресов хостинга, поэтому повторяем с паузой
        for attempt in range(5):
            req = urllib.request.Request(url, headers={"x-api-key": key, "Accept": "application/json", "User-Agent": "Noxel/0.2"})
            try:
                with urllib.request.urlopen(req, timeout=15) as r:
                    code, body = r.status, r.read(2_000_000)
            except urllib.error.HTTPError as e:
                code, body = e.code, e.read(200_000)
            except Exception:
                code = 0
            if code in (200, 400, 404):
                break
            time.sleep(0.8 * (attempt + 1))
        if code == 200:
            if len(_cf_cache) > 400:
                _cf_cache.clear()
            _cf_cache[ck] = (time.time(), body)
            return self.send(200, raw=body)
        if code in (400, 404):
            return self.send(code, raw=body)
        if hit and time.time() - hit[0] < 6 * 3600:  # свежих данных нет, отдаём прошлые
            return self.send(200, raw=hit[1])
        raise Err(502, "CurseForge временно недоступен, попробуй чуть позже")

    # ── маршруты ──
    def route(self, method):
        u = urlparse(self.path)
        p, q = u.path.rstrip("/"), parse_qs(u.query)
        if not p.startswith("/noxel/v1"):
            raise Err(404, "не найдено")
        p = p[len("/noxel/v1"):] or "/"
        if method == "GET" and p == "/ping":
            return self.send(200, {"ok": True, "service": "noxel", "time": int(time.time())})
        m = re.match(r"^/profile/([A-Za-z0-9_]{3,16})$", p)
        if method == "GET" and m:
            a = db().execute("SELECT * FROM accounts WHERE name=?", (m.group(1),)).fetchone()
            if not a:
                raise Err(404, "игрок не найден")
            return self.send(200, self.view(a))
        if method == "POST" and p in ("/register", "/login"):
            if not throttle(("auth", self.ip()), 10):
                raise Err(429, "слишком много попыток, подожди минуту")
            j = self.jbody()
            name, pw = str(j.get("name", "")).strip(), str(j.get("password", ""))
            if not NAME_RE.match(name):
                raise Err(400, "ник: от 3 до 16 символов, латиница, цифры и _")
            if p == "/register":
                if not (6 <= len(pw) <= 128):
                    raise Err(400, "пароль: от 6 до 128 символов")
                salt = secrets.token_bytes(16)
                try:
                    c = db()
                    cur = c.execute("INSERT INTO accounts(name,uuid,salt,pw,created) VALUES(?,?,?,?,?)", (name, offline_uuid(name), salt, hash_pw(pw, salt), int(time.time())))
                    c.commit()
                except sqlite3.IntegrityError:
                    raise Err(409, "этот ник уже занят")
                a = db().execute("SELECT * FROM accounts WHERE id=?", (cur.lastrowid,)).fetchone()
            else:
                a = db().execute("SELECT * FROM accounts WHERE name=?", (name,)).fetchone()
                ok = a and hmac.compare_digest(hash_pw(pw, a["salt"]), a["pw"])
                if not ok:
                    raise Err(403, "неверный ник или пароль")
            return self.send(200, {"token": self.new_token(a["id"]), "account": self.view(a)})

        m = re.match(r"^/cf/(.+)$", p)
        if method == "GET" and m:
            return self.cf_proxy(m.group(1), u.query)
        if method == "GET" and p in ("/servers", "/launcher", "/news"):
            try:
                with open({"/servers": SERVERS, "/launcher": LAUNCHER, "/news": NEWS}[p], "rb") as f:
                    return self.send(200, raw=f.read())
            except OSError:
                return self.send(200, {"/servers": {"version": 0, "servers": []}, "/news": {"version": 0, "items": []}}.get(p, {}))
        if method == "PUT" and p == "/news":
            tok = (self.headers.get("Authorization") or "")[7:].strip()
            admin = read_secret(ADMIN_FILE)
            if not admin or not hmac.compare_digest(tok.encode(), admin.encode()):
                throttle(("adm", self.ip()), 5)
                raise Err(403, "нужен токен администратора")
            data = clean_news(self.jbody())
            tmp = NEWS + ".tmp"
            with open(tmp, "w") as f:
                json.dump(data, f, ensure_ascii=False, indent=1)
            os.chmod(tmp, 0o644)
            os.replace(tmp, NEWS)
            return self.send(200, {"ok": True, "version": data["version"], "count": len(data["items"])})
        if method == "PUT" and p == "/servers":
            tok = (self.headers.get("Authorization") or "")[7:].strip()
            admin = read_secret(ADMIN_FILE)
            if not admin or not hmac.compare_digest(tok.encode(), admin.encode()):
                throttle(("adm", self.ip()), 5)
                raise Err(403, "нужен токен администратора")
            data = clean_servers(self.jbody())
            tmp = SERVERS + ".tmp"
            with open(tmp, "w") as f:
                json.dump(data, f, ensure_ascii=False, indent=1)
            os.chmod(tmp, 0o644)
            os.replace(tmp, SERVERS)
            return self.send(200, {"ok": True, "version": data["version"], "count": len(data["servers"])})

        a = self.auth()
        if method == "POST" and p == "/logout":
            t = (self.headers.get("Authorization") or "")[7:].strip()
            db().execute("DELETE FROM tokens WHERE hash=?", (hashlib.sha256(t.encode()).hexdigest(),))
            db().commit()
            return self.send(200, {"ok": True})
        if method == "GET" and p == "/me":
            return self.send(200, self.view(a))
        if p == "/data":
            if method == "GET":
                return self.send(200, json.loads(a["data"] or "{}"))
            if method == "PUT":
                raw = self.body()
                try:
                    json.loads(raw or b"{}")
                except ValueError:
                    raise Err(400, "неверный JSON")
                db().execute("UPDATE accounts SET data=? WHERE id=?", (raw.decode() or "{}", a["id"]))
                db().commit()
                return self.send(200, {"ok": True})
        if method == "POST" and p == "/password":
            j = self.jbody()
            if not hmac.compare_digest(hash_pw(str(j.get("old", "")), a["salt"]), a["pw"]):
                raise Err(403, "старый пароль неверный")
            new = str(j.get("new", ""))
            if not (6 <= len(new) <= 128):
                raise Err(400, "пароль: от 6 до 128 символов")
            salt = secrets.token_bytes(16)
            db().execute("UPDATE accounts SET salt=?, pw=? WHERE id=?", (salt, hash_pw(new, salt), a["id"]))
            db().execute("DELETE FROM tokens WHERE account=?", (a["id"],))
            db().commit()
            return self.send(200, {"token": self.new_token(a["id"])})
        if method == "DELETE" and p == "/account":
            j = self.jbody()
            if not hmac.compare_digest(hash_pw(str(j.get("password", "")), a["salt"]), a["pw"]):
                raise Err(403, "пароль неверный")
            db().execute("DELETE FROM accounts WHERE id=?", (a["id"],))
            db().commit()
            return self.send(200, {"ok": True})

        m = re.match(r"^/(skin|cape)(?:/(select|\d+))?$", p)
        if m:
            kind, sub = m.group(1), m.group(2)
            col = "skin_id" if kind == "skin" else "cape_id"
            if method == "GET" and not sub:
                return self.send(200, listing(a, kind))
            if method == "POST" and not sub:
                if not throttle(("up", a["id"]), 20):
                    raise Err(429, "слишком много загрузок, подожди минуту")
                raw = self.body()
                try:
                    png_size(raw, SKIN_SIZES if kind == "skin" else CAPE_SIZES)
                except ValueError as e:
                    raise Err(400, str(e))
                model = (q.get("model") or ["classic"])[0]
                model = "slim" if (kind == "skin" and model == "slim") else "classic"
                h = hashlib.sha256(raw).hexdigest()
                path = os.path.join(FILES, h + ".png")
                if not os.path.exists(path):
                    tmp = path + ".tmp"
                    with open(tmp, "wb") as f:
                        f.write(raw)
                    os.chmod(tmp, 0o644)
                    os.replace(tmp, path)
                c = db()
                c.execute("DELETE FROM files WHERE account=? AND kind=? AND hash=? AND model=?", (a["id"], kind, h, model))
                cur = c.execute("INSERT INTO files(account,kind,hash,model,created) VALUES(?,?,?,?,?)", (a["id"], kind, h, model, int(time.time())))
                c.execute("UPDATE accounts SET %s=? WHERE id=?" % col, (cur.lastrowid, a["id"]))
                c.commit()
                return self.send(200, self.view(c.execute("SELECT * FROM accounts WHERE id=?", (a["id"],)).fetchone()))
            if method == "POST" and sub == "select":
                fid = self.jbody().get("id")
                c = db()
                if fid is None:
                    if kind == "skin":
                        raise Err(400, "у скина нужен id")
                    c.execute("UPDATE accounts SET cape_id=NULL WHERE id=?", (a["id"],))
                else:
                    if not c.execute("SELECT 1 FROM files WHERE id=? AND account=? AND kind=?", (fid, a["id"], kind)).fetchone():
                        raise Err(404, "нет такой записи")
                    c.execute("UPDATE accounts SET %s=? WHERE id=?" % col, (fid, a["id"]))
                c.commit()
                return self.send(200, self.view(c.execute("SELECT * FROM accounts WHERE id=?", (a["id"],)).fetchone()))
            if method == "DELETE" and sub and sub != "select":
                c = db()
                c.execute("DELETE FROM files WHERE id=? AND account=? AND kind=?", (int(sub), a["id"], kind))
                c.execute("UPDATE accounts SET %s=NULL WHERE id=? AND %s=?" % (col, col), (a["id"], int(sub)))
                c.commit()
                return self.send(200, self.view(c.execute("SELECT * FROM accounts WHERE id=?", (a["id"],)).fetchone()))
        raise Err(404, "не найдено")

    def handle_any(self, method):
        try:
            self.route(method)
        except Err as e:
            try:
                self.send(e.code, {"error": e.msg})
            except OSError:
                pass
        except Exception as e:  # не роняем сервер и не показываем внутренности
            try:
                self.send(500, {"error": "внутренняя ошибка"})
            except OSError:
                pass
            print("ERR", type(e).__name__, e, flush=True)
        finally:
            # своё соединение на запрос: незавершённая транзакция не блокирует остальных
            c = getattr(_local, "c", None)
            if c is not None:
                try:
                    c.rollback()
                    c.close()
                except sqlite3.Error:
                    pass
                _local.c = None

    def do_GET(self): self.handle_any("GET")
    def do_POST(self): self.handle_any("POST")
    def do_PUT(self): self.handle_any("PUT")
    def do_DELETE(self): self.handle_any("DELETE")


if __name__ == "__main__":
    init()
    srv = ThreadingHTTPServer((HOST, PORT), H)
    srv.daemon_threads = True
    print("noxel api on %s:%d, base %s" % (HOST, PORT, BASE), flush=True)
    srv.serve_forever()
