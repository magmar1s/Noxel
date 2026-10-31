#!/usr/bin/env python3
"""Минимальный WebDAV-сервер для проверки лаунчера: python3 webdav_mini.py <порт> <папка> <логин:пароль>"""
import base64, os, sys, urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT, ROOT, CRED = int(sys.argv[1]), os.path.abspath(sys.argv[2]), sys.argv[3]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def path_fs(self):
        p = urllib.parse.unquote(urllib.parse.urlparse(self.path).path).strip("/")
        full = os.path.abspath(os.path.join(ROOT, p))
        return full if full.startswith(ROOT) else None

    def ok(self):
        h = self.headers.get("Authorization", "")
        if h == "Basic " + base64.b64encode(CRED.encode()).decode():
            return True
        self.send_response(401); self.send_header("WWW-Authenticate", 'Basic realm="x"'); self.send_header("Content-Length", "0"); self.end_headers()
        return False

    def reply(self, code, body=b"", ctype="text/plain"):
        self.send_response(code); self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)

    def do_MKCOL(self):
        if not self.ok(): return
        p = self.path_fs()
        if os.path.exists(p): return self.reply(405)
        os.makedirs(p); self.reply(201)

    def do_PUT(self):
        if not self.ok(): return
        p = self.path_fs(); n = int(self.headers.get("Content-Length") or 0)
        data = self.rfile.read(n); os.makedirs(os.path.dirname(p), exist_ok=True)
        open(p, "wb").write(data); self.reply(201)

    def do_GET(self):
        if not self.ok(): return
        p = self.path_fs()
        if not p or not os.path.isfile(p): return self.reply(404)
        self.reply(200, open(p, "rb").read(), "application/octet-stream")

    def do_PROPFIND(self):
        if not self.ok(): return
        n = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(n)
        p = self.path_fs()
        if not p or not os.path.exists(p): return self.reply(404)
        base = urllib.parse.urlparse(self.path).path
        items = [(base, p)]
        if os.path.isdir(p) and self.headers.get("Depth", "1") != "0":
            items += [(base.rstrip("/") + "/" + urllib.parse.quote(f), os.path.join(p, f)) for f in sorted(os.listdir(p))]
        out = '<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">'
        for href, fp in items:
            d = os.path.isdir(fp)
            out += "<D:response><D:href>%s</D:href><D:propstat><D:prop>%s%s<D:getlastmodified>Sat, 31 Oct 2026 12:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>" % (
                href + ("/" if d and not href.endswith("/") else ""), "<D:resourcetype><D:collection/></D:resourcetype>" if d else "<D:resourcetype/>",
                "" if d else "<D:getcontentlength>%d</D:getcontentlength>" % os.path.getsize(fp))
        out += "</D:multistatus>"
        self.reply(207, out.encode(), "application/xml; charset=utf-8")


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
