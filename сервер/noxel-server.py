#!/usr/bin/env python3
"""Noxel API: аккаунты с паролем, скины и плащи с историей, синхронизация данных.

Только стандартная библиотека Python 3. Слушает 127.0.0.1, наружу его выставляет nginx по HTTPS.
Пароли хранятся как scrypt-хеши, токены как sha256. Картинки лежат по хешу содержимого,
nginx отдаёт их напрямую по адресу {BASE}/f/<хеш>.png: этот адрес можно давать
SkinsRestorer командой /skin url.
"""
import hashlib, hmac, json, os, re, secrets, socket, socketserver, sqlite3, struct, threading, time, urllib.error, urllib.request, zlib
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
RECO = os.path.join(DATA, "recommended.json")
HORIZON = os.path.join(DATA, "horizon.json")
WORLDS = os.path.join(DATA, "worlds")
WORLD_MAX = 300 * 1024 * 1024          # размер мира в облаке
WORLD_QUOTA = WORLD_MAX                # в облаке Noxel хранится один мир на аккаунт (новый заменяет старый)
RELAY_PORT = int(os.environ.get("NOXEL_RELAY_PORT", "8731"))
RELAY_HOST = os.environ.get("NOXEL_RELAY_HOST", "mr1s.site")
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
    CREATE TABLE IF NOT EXISTS friends(
      a INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, b INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      status TEXT NOT NULL, created INTEGER NOT NULL, PRIMARY KEY(a, b));
    CREATE TABLE IF NOT EXISTS worlds(
      id INTEGER PRIMARY KEY, account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
      name TEXT NOT NULL, mc TEXT NOT NULL DEFAULT '', size INTEGER NOT NULL, created INTEGER NOT NULL, file TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS worlds_acc ON worlds(account, created);
    CREATE TABLE IF NOT EXISTS packcodes(
      code TEXT PRIMARY KEY, account INTEGER, created INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS shares(
      token TEXT PRIMARY KEY, world INTEGER NOT NULL REFERENCES worlds(id) ON DELETE CASCADE,
      account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, created INTEGER NOT NULL, downloads INTEGER NOT NULL DEFAULT 0);
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


def clean_reco(v):
    """Рекомендуемые моды и подборки: {"categories": [{id, title, desc, items: [mod | search | file]}]}."""
    if not isinstance(v, dict) or not isinstance(v.get("categories"), list) or len(v["categories"]) > 24:
        raise Err(400, "ожидается {\"categories\": [...]}, не больше 24 разделов")
    s = lambda x, n: str(x if x is not None else "").strip()[:n]
    cats = []
    for ci, c in enumerate(v["categories"]):
        if not isinstance(c, dict) or not s(c.get("title"), 60):
            raise Err(400, "раздел %d: нужен заголовок" % ci)
        items = []
        for ii, it in enumerate((c.get("items") or [])[:80]):
            if not isinstance(it, dict):
                raise Err(400, "раздел %d, запись %d: ожидается объект" % (ci, ii))
            t = it.get("type")
            if t == "mod":
                if it.get("provider") not in ("modrinth", "curseforge") or not s(it.get("id"), 60):
                    raise Err(400, "раздел %d, запись %d: нужны provider (modrinth/curseforge) и id" % (ci, ii))
                items.append({"type": "mod", "provider": it["provider"], "id": s(it["id"], 60), "name": s(it.get("name"), 80), "desc": s(it.get("desc"), 300), "tag": s(it.get("tag"), 30)})
            elif t == "search":
                if it.get("kind") not in ("mod", "resourcepack", "shader") or not s(it.get("query"), 80):
                    raise Err(400, "раздел %d, запись %d: нужны kind (mod/resourcepack/shader) и query" % (ci, ii))
                items.append({"type": "search", "title": s(it.get("title"), 80) or s(it["query"], 80), "desc": s(it.get("desc"), 300), "query": s(it["query"], 80), "kind": it["kind"]})
            elif t == "file":
                url = s(it.get("url"), 400)
                if not url.startswith("https://") or it.get("folder") not in ("emotes", "player_models", "resourcepacks", "shaderpacks"):
                    raise Err(400, "раздел %d, запись %d: url должен начинаться с https://, folder: emotes, player_models, resourcepacks или shaderpacks" % (ci, ii))
                items.append({"type": "file", "title": s(it.get("title"), 80), "desc": s(it.get("desc"), 300), "url": url, "folder": it["folder"], "extract": bool(it.get("extract")),
                              "prefix": s(it.get("prefix"), 120), "license": s(it.get("license"), 40), "author": s(it.get("author"), 60)})
            else:
                raise Err(400, "раздел %d, запись %d: type должен быть mod, search или file" % (ci, ii))
        cats.append({"id": re.sub(r"[^a-z0-9_-]", "", s(c.get("id"), 30).lower()) or "c%d" % ci, "title": s(c.get("title"), 60), "desc": s(c.get("desc"), 300), "items": items})
    return {"version": int(time.time()), "categories": cats}


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


# ── друзья, присутствие и ретранслятор «локальной сети» ──
_presence = {}            # id аккаунта -> {"ts", "game", "mc"}
_tickets = {}             # билет -> (id аккаунта, срок)
_tickets_lock = threading.Lock()
_relay_lock = threading.Lock()
_hosts = {}               # id аккаунта -> {"sock", "lock", "sessions"}
_pending = {}             # id сессии -> {"ev", "sock", "done"}
_flows = {}               # id UDP-потока (4 байта) -> {"g": адрес гостя, "h": адрес хозяина, "ts"}
_packs = {}               # id аккаунта -> сборка модов хозяина мира (для друзей)
ONLINE_SECS = 75


def ticket_new(acc_id):
    t = secrets.token_urlsafe(24)
    now = time.time()
    with _tickets_lock:
        for k in [k for k, v in _tickets.items() if v[1] < now]:
            _tickets.pop(k, None)
        _tickets[t] = (acc_id, now + 600)
    return t


def ticket_account(t):
    with _tickets_lock:
        v = _tickets.get(t)
    return v[0] if v and v[1] > time.time() else None


def are_friends(x, y):
    return db().execute("SELECT 1 FROM friends WHERE status='accepted' AND ((a=? AND b=?) OR (a=? AND b=?))", (x, y, y, x)).fetchone() is not None


def _close_db():
    c = getattr(_local, "c", None)
    if c is not None:
        try:
            c.close()
        except sqlite3.Error:
            pass
        _local.c = None


def _readline(s, limit=300):
    buf = b""
    while len(buf) < limit:
        ch = s.recv(1)
        if not ch:
            return None
        if ch == b"\n":
            return buf.decode("utf-8", "ignore").strip()
        buf += ch
    return None


def _pipe(a, b):
    def one(x, y):
        try:
            while True:
                d = x.recv(65536)
                if not d:
                    break
                y.sendall(d)
        except OSError:
            pass
        finally:
            for z in (x, y):
                try:
                    z.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
    t = threading.Thread(target=one, args=(b, a), daemon=True)
    t.start()
    one(a, b)
    t.join(3)


class Relay(socketserver.BaseRequestHandler):
    """Ретранслятор: хозяин мира держит управляющее соединение, гость подключается через сервер, байты пересылаются как есть.

    HOST <билет>            хозяин: «мой мир открыт», в ответ приходят строки CONN <сессия>
    DATA <сессия>           хозяин: канал данных для конкретного гостя (открывается в ответ на CONN)
    JOIN <билет> <ник>      гость: подключиться к миру друга (только принятые друзья)
    """

    def handle(self):
        s = self.request
        try:
            s.settimeout(15)
            s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            if not throttle(("relay", self.client_address[0]), 120):
                return
            parts = (_readline(s) or "").split()
            if len(parts) == 2 and parts[0] == "HOST":
                self.host(s, parts[1])
            elif len(parts) == 3 and parts[0] == "JOIN":
                self.join(s, parts[1], parts[2])
            elif len(parts) == 2 and parts[0] == "DATA":
                self.data(s, parts[1])
            elif len(parts) == 4 and parts[0] == "UDP" and parts[3].isdigit():
                self.udp(s, parts[1], parts[2], int(parts[3]))
        except OSError:
            pass
        finally:
            _close_db()
            try:
                s.close()
            except OSError:
                pass

    def host(self, s, ticket):
        acc = ticket_account(ticket)
        if acc is None:
            s.sendall(b"ERR ticket\n")
            return
        ctl = {"sock": s, "lock": threading.Lock(), "sessions": 0}
        with _relay_lock:
            old = _hosts.get(acc)
            _hosts[acc] = ctl
        if old:
            try:
                old["sock"].close()
            except OSError:
                pass
        s.sendall(b"OK\n")
        s.settimeout(60)                     # хозяин присылает PING каждые 20 секунд
        try:
            while s.recv(64):
                pass
        except OSError:
            pass
        finally:
            with _relay_lock:
                if _hosts.get(acc) is ctl:
                    del _hosts[acc]

    def join(self, s, ticket, host_name):
        guest = ticket_account(ticket)
        if guest is None:
            s.sendall(b"ERR ticket\n")
            return
        h = db().execute("SELECT id FROM accounts WHERE name=?", (host_name,)).fetchone()
        ok = h is not None and h["id"] != guest and are_friends(guest, h["id"])
        _close_db()
        if not ok:
            s.sendall(b"ERR friend\n")
            return
        ctl = _hosts.get(h["id"])
        if not ctl or ctl["sessions"] >= 16:
            s.sendall(b"ERR offline\n")
            return
        sid = secrets.token_hex(8)
        p = {"ev": threading.Event(), "sock": None, "done": threading.Event()}
        _pending[sid] = p
        try:
            with ctl["lock"]:
                ctl["sock"].sendall(("CONN %s\n" % sid).encode())
        except OSError:
            _pending.pop(sid, None)
            s.sendall(b"ERR offline\n")
            return
        if not p["ev"].wait(10):
            _pending.pop(sid, None)
            s.sendall(b"ERR timeout\n")
            return
        ctl["sessions"] += 1
        try:
            s.sendall(b"OK\n")
            s.settimeout(None)
            p["sock"].settimeout(None)
            _pipe(s, p["sock"])
        finally:
            ctl["sessions"] -= 1
            p["done"].set()

    def udp(self, s, ticket, host_name, port):
        """Гость просит UDP-поток к порту хозяина (голосовой чат и т. п.). Соединение живёт, пока живёт поток."""
        guest = ticket_account(ticket)
        if guest is None:
            s.sendall(b"ERR ticket\n")
            return
        h = db().execute("SELECT id FROM accounts WHERE name=?", (host_name,)).fetchone()
        ok = h is not None and h["id"] != guest and are_friends(guest, h["id"])
        _close_db()
        if not ok:
            s.sendall(b"ERR friend\n")
            return
        if port not in ((_presence.get(h["id"]) or {}).get("ports") or []):
            s.sendall(b"ERR port\n")
            return
        ctl = _hosts.get(h["id"])
        if not ctl:
            s.sendall(b"ERR offline\n")
            return
        fid = secrets.token_bytes(4)
        _flows[fid] = {"g": None, "h": None, "ts": time.time()}
        try:
            with ctl["lock"]:
                ctl["sock"].sendall(("UDPFLOW %s %d\n" % (fid.hex(), port)).encode())
            s.sendall(("OK %s\n" % fid.hex()).encode())
            s.settimeout(None)
            while s.recv(64):
                pass
        except OSError:
            pass
        finally:
            _flows.pop(fid, None)
            try:
                with ctl["lock"]:
                    ctl["sock"].sendall(("UDPEND %s\n" % fid.hex()).encode())
            except OSError:
                pass

    def data(self, s, sid):
        p = _pending.pop(sid, None)
        if not p:
            return
        p["sock"] = s
        p["ev"].set()
        p["done"].wait(12 * 3600)            # сокет закроется, когда гость отключится


def udp_loop():
    """UDP-ретранслятор: пакеты вида <поток 4 байта><роль G/H><данные> пересылаются между гостем и хозяином потока."""
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    u.bind(("0.0.0.0", RELAY_PORT))
    last_clean = time.time()
    while True:
        try:
            data, addr = u.recvfrom(2048)
        except OSError:
            continue
        if len(data) < 5:
            continue
        f = _flows.get(data[:4])
        role = data[4:5]
        if f is None or role not in (b"G", b"H"):
            continue
        key, other, mark = ("g", "h", b"H") if role == b"G" else ("h", "g", b"G")
        f[key] = addr
        f["ts"] = time.time()
        if f[other] and len(data) > 5:
            try:
                u.sendto(data[:4] + mark + data[5:], f[other])
            except OSError:
                pass
        if time.time() - last_clean > 60:
            last_clean = time.time()
            for k in [k for k, v in _flows.items() if time.time() - v["ts"] > 900]:
                _flows.pop(k, None)


def start_relay():
    class Srv(socketserver.ThreadingTCPServer):
        allow_reuse_address = True
        daemon_threads = True
        request_queue_size = 64
    srv = Srv(("0.0.0.0", RELAY_PORT), Relay)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    threading.Thread(target=udp_loop, daemon=True).start()
    print("noxel relay on 0.0.0.0:%d (tcp+udp)" % RELAY_PORT, flush=True)


PACK_ALPHA = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ"


def clean_pack(j):
    """Описание сборки для кода: только ожидаемые поля и разумные размеры."""
    items = []
    for m in (j.get("items") or [])[:800]:
        if not isinstance(m, dict):
            continue
        prov, mid, kind = str(m.get("provider", "")), str(m.get("id", "")), str(m.get("kind", "mod"))
        if prov not in ("modrinth", "curseforge") or kind not in ("mod", "resourcepack", "shader") or not re.match(r"^[A-Za-z0-9]{1,40}$", mid):
            continue
        it = {"provider": prov, "id": mid, "kind": kind, "name": str(m.get("name", ""))[:80]}
        v = m.get("version")
        if isinstance(v, str) and re.match(r"^[A-Za-z0-9]{1,40}$", v):
            it["version"] = v
        items.append(it)
    cfg = j.get("config")
    if not (isinstance(cfg, str) and len(cfg) <= 420_000 and re.match(r"^[A-Za-z0-9+/=]*$", cfg)):
        cfg = None
    loader = str(j.get("loader", "vanilla"))
    if loader not in ("vanilla", "fabric", "quilt", "forge", "neoforge", "optifine"):
        raise Err(400, "неизвестный загрузчик")
    mc = str(j.get("mc", ""))[:24]
    if not re.match(r"^[A-Za-z0-9.\- _]+$", mc):
        raise Err(400, "неверная версия игры")
    lv = j.get("loader_ver")
    return {"name": str(j.get("name", ""))[:60], "mc": mc, "loader": loader, "loader_ver": str(lv)[:40] if lv else None, "items": items, "config": cfg}


# ── Yggdrasil (authlib-injector): вход аккаунтом Noxel в игре, скины видны всем ──
YGG_KEY = os.path.join(DATA, "ygg.key")
_joins = {}
_sig_cache = {}


def ygg_key():
    if not os.path.exists(YGG_KEY):
        import subprocess
        subprocess.run(["openssl", "genrsa", "-out", YGG_KEY, "4096"], check=True, capture_output=True)
        os.chmod(YGG_KEY, 0o600)
    return YGG_KEY


def ygg_pub():
    import subprocess
    return subprocess.run(["openssl", "rsa", "-in", ygg_key(), "-pubout"], check=True, capture_output=True).stdout.decode()


def ygg_sign(value):
    import base64, subprocess
    if value in _sig_cache:
        return _sig_cache[value]
    sig = subprocess.run(["openssl", "dgst", "-sha1", "-sign", ygg_key()], input=value.encode(), check=True, capture_output=True).stdout
    if len(_sig_cache) > 5000:
        _sig_cache.clear()
    _sig_cache[value] = base64.b64encode(sig).decode()
    return _sig_cache[value]


def ygg_profile(a, signed):
    import base64
    tex = {}
    if a["skin_id"]:
        f = db().execute("SELECT * FROM files WHERE id=?", (a["skin_id"],)).fetchone()
        if f:
            tex["SKIN"] = {"url": "%s/f/%s.png" % (BASE, f["hash"])}
            if f["model"] == "slim":
                tex["SKIN"]["metadata"] = {"model": "slim"}
    if a["cape_id"]:
        f = db().execute("SELECT * FROM files WHERE id=?", (a["cape_id"],)).fetchone()
        if f:
            tex["CAPE"] = {"url": "%s/f/%s.png" % (BASE, f["hash"])}
    # время с точностью до часа: значение и подпись повторяются и кэшируются
    ts = int(time.time() // 3600 * 3600 * 1000)
    val = base64.b64encode(json.dumps({"timestamp": ts, "profileId": a["uuid"], "profileName": a["name"], "textures": tex}, separators=(",", ":")).encode()).decode()
    prop = {"name": "textures", "value": val}
    if signed:
        prop["signature"] = ygg_sign(val)
    return {"id": a["uuid"], "name": a["name"], "properties": [prop]}


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
        if n > MAX_BODY and not self.path.startswith(("/noxel/v1/data", "/noxel/v1/servers", "/noxel/v1/recommended", "/noxel/v1/packs")):
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

    def share_public(self, token, download):
        """Общая ссылка на облачный мир: сведения для страницы и скачивание без входа."""
        if not throttle(("share", self.ip()), 30 if not download else 6):
            raise Err(429, "слишком много запросов, подожди минуту")
        r = db().execute("SELECT s.token, s.downloads, w.id wid, w.name, w.mc, w.size, w.created, w.file, w.account, a.name owner FROM shares s JOIN worlds w ON w.id=s.world JOIN accounts a ON a.id=s.account WHERE s.token=?", (token,)).fetchone()
        if not r:
            raise Err(404, "ссылка недействительна: автор удалил мир или отозвал ссылку")
        if not download:
            return self.send(200, {"name": r["name"], "mc": r["mc"], "size": r["size"], "created": r["created"], "owner": r["owner"], "downloads": r["downloads"]})
        try:
            f = open(os.path.join(WORLDS, str(r["account"]), r["file"]), "rb")
        except OSError:
            raise Err(404, "файл мира потерян")
        db().execute("UPDATE shares SET downloads=downloads+1 WHERE token=?", (token,))
        db().commit()
        with f:
            self.send_response(200)
            self.send_header("Content-Type", "application/zip")
            self.send_header("Content-Length", str(r["size"]))
            self.send_header("Content-Disposition", "attachment; filename=\"noxel-world.zip\"")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            while True:
                chunk = f.read(65536)
                if not chunk:
                    break
                self.wfile.write(chunk)

    def cf_batch(self):
        """Пакетный запрос описаний файлов CurseForge (для установки сборок): POST /cf/mods/files {"fileIds": [...]}."""
        key = read_secret(CF_KEY_FILE)
        if not key:
            raise Err(503, "CurseForge на сервере не настроен")
        ids = self.jbody().get("fileIds")
        if not isinstance(ids, list) or not ids or len(ids) > 300 or not all(isinstance(i, int) and 0 < i < 10**10 for i in ids):
            raise Err(400, "ожидается {\"fileIds\": [до 300 чисел]}")
        if not throttle(("cfb", self.ip()), 20):
            raise Err(429, "слишком много запросов, подожди минуту")
        data = json.dumps({"fileIds": ids}).encode()
        code, body = 0, b""
        for attempt in range(5):
            req = urllib.request.Request("https://api.curseforge.com/v1/mods/files", data=data, method="POST",
                                         headers={"x-api-key": key, "Accept": "application/json", "Content-Type": "application/json", "User-Agent": "Noxel/0.6"})
            try:
                with urllib.request.urlopen(req, timeout=25) as r:
                    code, body = r.status, r.read(8_000_000)
            except urllib.error.HTTPError as e:
                code, body = e.code, e.read(200_000)
            except Exception:
                code = 0
            if code in (200, 400, 404):
                break
            time.sleep(0.8 * (attempt + 1))
        if code in (200, 400, 404):
            return self.send(code, raw=body)
        raise Err(502, "CurseForge временно недоступен, попробуй чуть позже")

    def cf_fingerprints(self):
        """Поиск файлов CurseForge по отпечаткам (для экспорта сборки в формат CurseForge)."""
        key = read_secret(CF_KEY_FILE)
        if not key:
            raise Err(503, "CurseForge на сервере не настроен")
        fps = self.jbody().get("fingerprints")
        if not isinstance(fps, list) or not fps or len(fps) > 400 or not all(isinstance(i, int) and 0 <= i < 2**32 for i in fps):
            raise Err(400, "ожидается {\"fingerprints\": [до 400 чисел]}")
        if not throttle(("cff", self.ip()), 10):
            raise Err(429, "слишком много запросов, подожди минуту")
        req = urllib.request.Request("https://api.curseforge.com/v1/fingerprints/432", data=json.dumps({"fingerprints": fps}).encode(), method="POST",
                                     headers={"x-api-key": key, "Accept": "application/json", "Content-Type": "application/json", "User-Agent": "Noxel/0.6"})
        try:
            with urllib.request.urlopen(req, timeout=25) as r:
                return self.send(200, raw=r.read(8_000_000))
        except urllib.error.HTTPError as e:
            raise Err(502, "CurseForge ответил %d" % e.code)
        except Exception:
            raise Err(502, "CurseForge временно недоступен")

    def ygg_fail(self, code, msg):
        self.send(code, {"error": "ForbiddenOperationException" if code == 403 else "IllegalArgumentException", "errorMessage": msg})

    def ygg(self, method, p, q):
        """API для authlib-injector: https://github.com/yushijinhun/authlib-injector/wiki"""
        if method == "GET" and p == "/":
            site = BASE.rsplit("/noxel", 1)[0]
            host = urlparse(site).hostname or "mr1s.site"
            return self.send(200, {"meta": {"serverName": "Noxel", "implementationName": "noxel-server", "implementationVersion": "1",
                                            "links": {"homepage": site, "register": site}, "feature.non_email_login": True},
                                   "skinDomains": [host, "." + host], "signaturePublickey": ygg_pub()})
        if method == "POST" and p in ("/authserver/authenticate", "/authserver/signout"):
            if not throttle(("yauth", self.ip()), 10):
                return self.ygg_fail(403, "Too many attempts")
            j = self.jbody()
            a = db().execute("SELECT * FROM accounts WHERE name=?", (str(j.get("username", "")),)).fetchone()
            if not a or not hmac.compare_digest(hash_pw(str(j.get("password", "")), a["salt"]), a["pw"]):
                return self.ygg_fail(403, "Invalid credentials. Invalid username or password.")
            if p.endswith("signout"):
                db().execute("DELETE FROM tokens WHERE account=?", (a["id"],))
                db().commit()
                return self.send(204, raw=b"")
            prof = {"id": a["uuid"], "name": a["name"]}
            ct = str(j.get("clientToken") or secrets.token_hex(16))[:64]
            out = {"accessToken": self.new_token(a["id"]), "clientToken": ct, "availableProfiles": [prof], "selectedProfile": prof}
            if j.get("requestUser"):
                out["user"] = {"id": a["uuid"], "properties": []}
            return self.send(200, out)
        if method == "POST" and p in ("/authserver/refresh", "/authserver/validate", "/authserver/invalidate"):
            j = self.jbody()
            h = hashlib.sha256(str(j.get("accessToken", "")).encode()).hexdigest()
            r = db().execute("SELECT a.* FROM tokens t JOIN accounts a ON a.id=t.account WHERE t.hash=?", (h,)).fetchone()
            if p.endswith("invalidate"):
                db().execute("DELETE FROM tokens WHERE hash=?", (h,))
                db().commit()
                return self.send(204, raw=b"")
            if not r:
                return self.ygg_fail(403, "Invalid token.")
            if p.endswith("validate"):
                return self.send(204, raw=b"")
            db().execute("DELETE FROM tokens WHERE hash=?", (h,))
            db().commit()
            prof = {"id": r["uuid"], "name": r["name"]}
            out = {"accessToken": self.new_token(r["id"]), "clientToken": str(j.get("clientToken") or "")[:64], "selectedProfile": prof}
            if j.get("requestUser"):
                out["user"] = {"id": r["uuid"], "properties": []}
            return self.send(200, out)
        if method == "POST" and p == "/sessionserver/session/minecraft/join":
            j = self.jbody()
            h = hashlib.sha256(str(j.get("accessToken", "")).encode()).hexdigest()
            r = db().execute("SELECT a.* FROM tokens t JOIN accounts a ON a.id=t.account WHERE t.hash=?", (h,)).fetchone()
            if not r or str(j.get("selectedProfile", "")).replace("-", "") != r["uuid"]:
                return self.ygg_fail(403, "Invalid token.")
            sid = str(j.get("serverId", ""))[:64]
            now = time.time()
            for k in [k for k, v in _joins.items() if now - v[2] > 30]:
                _joins.pop(k, None)
            _joins[sid] = (r["id"], self.ip(), now)
            return self.send(204, raw=b"")
        if method == "GET" and p == "/sessionserver/session/minecraft/hasJoined":
            sid, name = (q.get("serverId") or [""])[0], (q.get("username") or [""])[0]
            j = _joins.get(sid)
            if not j or time.time() - j[2] > 30:
                return self.send(204, raw=b"")
            a = db().execute("SELECT * FROM accounts WHERE id=?", (j[0],)).fetchone()
            if not a or a["name"].lower() != name.lower():
                return self.send(204, raw=b"")
            return self.send(200, ygg_profile(a, True))
        m = re.match(r"^/sessionserver/session/minecraft/profile/([0-9a-fA-F-]{32,36})$", p)
        if method == "GET" and m:
            a = db().execute("SELECT * FROM accounts WHERE uuid=?", (m.group(1).replace("-", "").lower(),)).fetchone()
            if not a:
                return self.send(204, raw=b"")
            return self.send(200, ygg_profile(a, (q.get("unsigned") or ["true"])[0] == "false"))
        if method == "POST" and p in ("/api/profiles/minecraft", "/api/profiles/minecraft/"):
            try:
                names = json.loads(self.body() or b"[]")
            except ValueError:
                names = []
            out = []
            for n in (names if isinstance(names, list) else [])[:10]:
                a = db().execute("SELECT uuid, name FROM accounts WHERE name=?", (str(n),)).fetchone()
                if a:
                    out.append({"id": a["uuid"], "name": a["name"]})
            return self.send(200, raw=json.dumps(out).encode())
        m = re.match(r"^/api/users/profiles/minecraft/([A-Za-z0-9_]{1,16})$", p)
        if method == "GET" and m:
            a = db().execute("SELECT uuid, name FROM accounts WHERE name=?", (m.group(1),)).fetchone()
            return self.send(200, {"id": a["uuid"], "name": a["name"]}) if a else self.send(204, raw=b"")
        return self.ygg_fail(404, "Not found")

    def social(self, method, p, a):
        """Друзья, присутствие, билеты ретранслятора и облачные миры. True, если запрос обработан."""
        now = time.time()
        if method == "GET" and p == "/friends":
            rows = db().execute("SELECT f.a, f.status, x.id xid, x.name xname FROM friends f JOIN accounts x ON x.id = CASE WHEN f.a=? THEN f.b ELSE f.a END WHERE f.a=? OR f.b=?", (a["id"], a["id"], a["id"])).fetchall()
            friends, incoming, outgoing = [], [], []
            for r in rows:
                if r["status"] == "accepted":
                    pr = _presence.get(r["xid"])
                    online = bool(pr and now - pr["ts"] < ONLINE_SECS)
                    friends.append({"name": r["xname"], "online": online, "game": pr["game"] if online else None, "mc": pr["mc"] if online else None,
                                    "room": online and r["xid"] in _hosts, "ports": (pr.get("ports") or []) if online else [], "pack": r["xid"] in _packs})
                elif r["a"] == a["id"]:
                    outgoing.append(r["xname"])
                else:
                    incoming.append(r["xname"])
            friends.sort(key=lambda f: (not f["online"], f["name"].lower()))
            self.send(200, {"friends": friends, "incoming": incoming, "outgoing": outgoing, "me": a["name"]})
            return True
        if method == "POST" and p in ("/friends", "/friends/accept", "/friends/remove"):
            if not throttle(("fr", a["id"]), 40):
                raise Err(429, "слишком много запросов")
            name = str(self.jbody().get("name", "")).strip()
            o = db().execute("SELECT id, name FROM accounts WHERE name=?", (name,)).fetchone() if NAME_RE.match(name) else None
            if not o:
                raise Err(404, "игрок с таким ником не найден в Noxel")
            if o["id"] == a["id"]:
                raise Err(400, "это ты")
            c = db()
            mine = c.execute("SELECT status FROM friends WHERE a=? AND b=?", (a["id"], o["id"])).fetchone()
            theirs = c.execute("SELECT status FROM friends WHERE a=? AND b=?", (o["id"], a["id"])).fetchone()
            if p == "/friends/remove":
                c.execute("DELETE FROM friends WHERE (a=? AND b=?) OR (a=? AND b=?)", (a["id"], o["id"], o["id"], a["id"]))
                state = "removed"
            elif mine or (theirs and theirs["status"] == "accepted"):
                state = "accepted" if (mine and mine["status"] == "accepted") or theirs else "pending"
            elif theirs:                                   # встречная заявка: становимся друзьями
                c.execute("UPDATE friends SET status='accepted' WHERE a=? AND b=?", (o["id"], a["id"]))
                state = "accepted"
            elif p == "/friends/accept":
                raise Err(404, "заявки от этого игрока нет")
            else:
                if c.execute("SELECT COUNT(*) FROM friends WHERE a=? OR b=?", (a["id"], a["id"])).fetchone()[0] >= 200:
                    raise Err(400, "слишком много друзей")
                c.execute("INSERT INTO friends(a,b,status,created) VALUES(?,?,?,?)", (a["id"], o["id"], "pending", int(now)))
                state = "pending"
            c.commit()
            self.send(200, {"ok": True, "state": state, "name": o["name"]})
            return True
        if method == "POST" and p == "/presence":
            j = self.jbody()
            g, mc = j.get("game"), j.get("mc")
            ports = [x for x in (j.get("ports") or []) if isinstance(x, int) and 1024 <= x <= 65535][:4] if isinstance(j.get("ports"), list) else []
            _presence[a["id"]] = {"ts": now, "game": str(g)[:40] if g else None, "mc": str(mc)[:20] if mc else None, "ports": ports}
            if len(_presence) > 20000:
                for k in [k for k, v in _presence.items() if now - v["ts"] > 3600]:
                    _presence.pop(k, None)
            self.send(200, {"ok": True, "room": a["id"] in _hosts})
            return True
        if method == "POST" and p == "/pack":
            j = self.jbody()
            mods = [{"provider": str(m.get("provider", ""))[:12], "id": str(m.get("id", ""))[:40], "name": str(m.get("name", ""))[:80]} for m in (j.get("mods") or []) if isinstance(m, dict)][:600]
            _packs[a["id"]] = {"mc": str(j.get("mc", ""))[:20], "loader": str(j.get("loader", ""))[:12], "loader_ver": str(j.get("loader_ver") or "")[:40], "mods": mods, "name": str(j.get("name", ""))[:60]}
            self.send(200, {"ok": True, "mods": len(mods)})
            return True
        m = re.match(r"^/pack/([A-Za-z0-9_]{3,16})$", p)
        if method == "GET" and m:
            o = db().execute("SELECT id FROM accounts WHERE name=?", (m.group(1),)).fetchone()
            if not o or not are_friends(a["id"], o["id"]) or o["id"] not in _packs:
                raise Err(404, "у друга нет опубликованной сборки")
            self.send(200, _packs[o["id"]])
            return True
        if method == "POST" and p == "/relay/ticket":
            if not throttle(("tk", a["id"]), 60):
                raise Err(429, "слишком много запросов")
            self.send(200, {"ticket": ticket_new(a["id"]), "host": RELAY_HOST, "port": RELAY_PORT})
            return True
        # облачные миры
        if method == "GET" and p == "/worlds":
            rows = db().execute("SELECT w.id, w.name, w.mc, w.size, w.created, s.token, COALESCE(s.downloads,0) downloads FROM worlds w LEFT JOIN shares s ON s.world=w.id WHERE w.account=? ORDER BY w.created DESC", (a["id"],)).fetchall()
            base = BASE.rsplit("/noxel", 1)[0]
            self.send(200, {"worlds": [dict(id=r["id"], name=r["name"], mc=r["mc"], size=r["size"], created=r["created"], downloads=r["downloads"], share=("%s/w/%s" % (base, r["token"])) if r["token"] else None) for r in rows],
                            "used": sum(r["size"] for r in rows), "quota": WORLD_QUOTA, "max": WORLD_MAX})
            return True
        if method == "POST" and p == "/world":
            q = parse_qs(urlparse(self.path).query)
            name = (q.get("name", [""])[0]).strip()[:64]
            mc = (q.get("mc", [""])[0]).strip()[:20]
            n = int(self.headers.get("Content-Length") or 0)
            self.close_connection = True
            if not name or n <= 0:
                raise Err(400, "нужны имя мира и файл")
            if n > WORLD_MAX:
                raise Err(413, "мир больше %d МБ" % (WORLD_MAX // 1048576))
            d = os.path.join(WORLDS, str(a["id"]))
            os.makedirs(d, exist_ok=True)
            fid = secrets.token_hex(12) + ".zip"
            tmp = os.path.join(d, fid + ".part")
            left, head = n, b""
            with open(tmp, "wb") as f:
                while left:
                    chunk = self.rfile.read(min(65536, left))
                    if not chunk:
                        break
                    if not head:
                        head = chunk[:4]
                    f.write(chunk)
                    left -= len(chunk)
            if left or head[:2] != b"PK":
                os.remove(tmp)
                raise Err(400, "загрузка прервана или это не архив мира")
            os.replace(tmp, os.path.join(d, fid))
            c = db()
            c.execute("INSERT INTO worlds(account,name,mc,size,created,file) VALUES(?,?,?,?,?,?)", (a["id"], name, mc, n, int(now), fid))
            new_id = c.execute("SELECT last_insert_rowid()").fetchone()[0]
            old = c.execute("SELECT id, file FROM worlds WHERE account=? AND id<>?", (a["id"], new_id)).fetchall()
            for r in old:                                  # в облаке Noxel один мир: новый заменяет прежний
                c.execute("DELETE FROM worlds WHERE id=?", (r["id"],))
                try:
                    os.remove(os.path.join(d, r["file"]))
                except OSError:
                    pass
            c.commit()
            self.close_connection = False
            self.send(200, {"ok": True, "size": n})
            return True
        m = re.match(r"^/world/(\d+)/share$", p)
        if m and method in ("POST", "DELETE"):
            r = db().execute("SELECT id FROM worlds WHERE id=? AND account=?", (int(m.group(1)), a["id"])).fetchone()
            if not r:
                raise Err(404, "мира нет в облаке")
            if method == "DELETE":
                db().execute("DELETE FROM shares WHERE world=?", (r["id"],))
                db().commit()
                return self.send(200, {"ok": True}) or True
            row = db().execute("SELECT token FROM shares WHERE world=?", (r["id"],)).fetchone()
            token = row["token"] if row else secrets.token_urlsafe(12)
            if not row:
                db().execute("INSERT INTO shares(token,world,account,created) VALUES(?,?,?,?)", (token, r["id"], a["id"], int(now)))
                db().commit()
            self.send(200, {"token": token, "url": "%s/w/%s" % (BASE.rsplit("/noxel", 1)[0], token)})
            return True
        m = re.match(r"^/world/(\d+)$", p)
        if m and method in ("GET", "DELETE"):
            r = db().execute("SELECT * FROM worlds WHERE id=? AND account=?", (int(m.group(1)), a["id"])).fetchone()
            if not r:
                raise Err(404, "мира нет в облаке")
            path = os.path.join(WORLDS, str(a["id"]), r["file"])
            if method == "DELETE":
                db().execute("DELETE FROM worlds WHERE id=?", (r["id"],))
                db().commit()
                try:
                    os.remove(path)
                except OSError:
                    pass
                self.send(200, {"ok": True})
                return True
            try:
                f = open(path, "rb")
            except OSError:
                raise Err(404, "файл мира потерян")
            with f:
                self.send_response(200)
                self.send_header("Content-Type", "application/zip")
                self.send_header("Content-Length", str(r["size"]))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                while True:
                    chunk = f.read(65536)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
            return True
        return False

    # ── маршруты ──
    def route(self, method):
        u = urlparse(self.path)
        p, q = u.path.rstrip("/"), parse_qs(u.query)
        if p == "/noxel/ygg" or p.startswith("/noxel/ygg/"):
            return self.ygg(method, p[len("/noxel/ygg"):] or "/", q)
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

        m = re.match(r"^/share/([A-Za-z0-9_-]{10,40})(/download)?$", p)
        if method == "GET" and m:
            return self.share_public(m.group(1), bool(m.group(2)))
        if method == "POST" and p == "/cf/mods/files":
            return self.cf_batch()
        if method == "POST" and p == "/cf/fingerprints":
            return self.cf_fingerprints()
        if method == "POST" and p == "/packs":
            if not throttle(("pk", self.ip()), 12):
                raise Err(429, "слишком часто, подожди минуту")
            acc = None
            if (self.headers.get("Authorization") or "").startswith("Bearer "):
                try:
                    acc = self.auth()["id"]
                except Err:
                    acc = None
            data = clean_pack(self.jbody())
            c = db()
            c.execute("DELETE FROM packcodes WHERE created < ? AND used < ?", (int(time.time()) - 180 * 86400, int(time.time()) - 60 * 86400))
            for _ in range(20):
                code = "".join(secrets.choice(PACK_ALPHA) for _ in range(8))
                try:
                    c.execute("INSERT INTO packcodes(code,account,created,used,data) VALUES(?,?,?,?,?)", (code, acc, int(time.time()), int(time.time()), json.dumps(data, ensure_ascii=False)))
                    break
                except sqlite3.IntegrityError:
                    continue
            c.commit()
            return self.send(200, {"code": code, "url": "%s/p/%s" % (BASE.rsplit("/noxel", 1)[0], code)})
        m = re.match(r"^/packs/([A-Za-z0-9]{6,12})$", p)
        if method == "GET" and m:
            if not throttle(("pkg", self.ip()), 40):
                raise Err(429, "слишком часто, подожди минуту")
            r = db().execute("SELECT data, created FROM packcodes WHERE code=?", (m.group(1).upper(),)).fetchone()
            if not r:
                raise Err(404, "сборка с таким кодом не найдена")
            db().execute("UPDATE packcodes SET used=? WHERE code=?", (int(time.time()), m.group(1).upper()))
            db().commit()
            d = json.loads(r["data"])
            d["created"] = r["created"]
            return self.send(200, d)
        m = re.match(r"^/cf/(.+)$", p)
        if method == "GET" and m:
            return self.cf_proxy(m.group(1), u.query)
        if method == "GET" and p in ("/servers", "/launcher", "/news", "/recommended", "/horizon"):
            try:
                with open({"/servers": SERVERS, "/launcher": LAUNCHER, "/news": NEWS, "/recommended": RECO, "/horizon": HORIZON}[p], "rb") as f:
                    return self.send(200, raw=f.read())
            except OSError:
                return self.send(200, {"/servers": {"version": 0, "servers": []}, "/news": {"version": 0, "items": []}, "/recommended": {"version": 0, "categories": []}, "/horizon": {"version": 0, "launcher": None, "builds": []}}.get(p, {}))
        if method == "PUT" and p == "/recommended":
            tok = (self.headers.get("Authorization") or "")[7:].strip()
            admin = read_secret(ADMIN_FILE)
            if not admin or not hmac.compare_digest(tok.encode(), admin.encode()):
                throttle(("adm", self.ip()), 5)
                raise Err(403, "нужен токен администратора")
            data = clean_reco(self.jbody())
            tmp = RECO + ".tmp"
            with open(tmp, "w") as f:
                json.dump(data, f, ensure_ascii=False, indent=1)
            os.chmod(tmp, 0o644)
            os.replace(tmp, RECO)
            return self.send(200, {"ok": True, "version": data["version"], "count": len(data["categories"])})
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
        if method == "POST" and p == "/ygg/token":
            # отдельный токен для игры: в процесс игры не уходит основной токен лаунчера
            return self.send(200, {"accessToken": self.new_token(a["id"]), "uuid": a["uuid"], "name": a["name"]})
        if self.social(method, p, a):
            return
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
            db().execute("DELETE FROM packcodes WHERE account=?", (a["id"],))
            db().execute("DELETE FROM accounts WHERE id=?", (a["id"],))
            db().commit()
            _presence.pop(a["id"], None)
            _packs.pop(a["id"], None)
            import shutil
            shutil.rmtree(os.path.join(WORLDS, str(a["id"])), ignore_errors=True)
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


def horizon_refresh():
    """Релизы проекта Horizon (Minecraft на Rust) с GitHub: лаунчер Horizon по платформам и список сборок игры. Игроки берут список у нас, поэтому новые версии появляются у всех сами."""
    req = urllib.request.Request("https://api.github.com/repos/horizon-rust/horizon/releases?per_page=40", headers={"User-Agent": "Noxel-feed", "Accept": "application/vnd.github+json"})
    rel = json.load(urllib.request.urlopen(req, timeout=30))
    launcher, builds = None, []
    for r in rel:
        tag = r.get("tag_name", "")
        if r.get("draft"):
            continue
        if tag.startswith("launcher@"):
            if launcher is None and not r.get("prerelease"):
                assets = {}
                for a in r.get("assets", []):
                    n, dg = a.get("name", ""), (a.get("digest") or "")
                    if not dg.startswith("sha256:"):
                        continue
                    key = "linux-x86_64" if n == "horizon-launcher-linux-x86_64.AppImage" else "windows-x64" if n == "horizon-launcher-windows-x64.zip" else None
                    if key:
                        assets[key] = {"name": n, "url": a["browser_download_url"], "sha256": dg[7:], "size": a.get("size", 0)}
                if assets:
                    launcher = {"version": tag.split("@", 1)[1], "date": (r.get("published_at") or "")[:10], "assets": assets}
        elif len(builds) < 8:
            builds.append({"tag": tag, "name": r.get("name") or tag, "nightly": bool(r.get("prerelease")), "date": (r.get("published_at") or "")[:10]})
    if launcher:
        tmp = HORIZON + ".tmp"
        with open(tmp, "w") as f:
            json.dump({"version": int(time.time()), "launcher": launcher, "builds": builds, "site": "https://playhorizon.dev", "repo": "https://github.com/horizon-rust/horizon"}, f, ensure_ascii=False)
        os.replace(tmp, HORIZON)
        os.chmod(HORIZON, 0o644)


def horizon_loop():
    while True:
        try:
            horizon_refresh()
        except Exception as e:
            print("horizon feed:", e, flush=True)
        time.sleep(1800)


if __name__ == "__main__":
    init()
    threading.Thread(target=horizon_loop, daemon=True).start()
    start_relay()
    srv = ThreadingHTTPServer((HOST, PORT), H)
    srv.daemon_threads = True
    print("noxel api on %s:%d, base %s" % (HOST, PORT, BASE), flush=True)
    srv.serve_forever()
