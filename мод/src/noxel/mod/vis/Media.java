package noxel.mod.vis;

import com.google.gson.JsonObject;
import java.util.concurrent.CompletableFuture;
import noxel.mod.Api;

/** Плеер: название и прогресс трека берёт у лаунчера Noxel, который читает системный медиаплеер. */
final class Media {
    private Media() {}

    static volatile String title = "", artist = "";
    static volatile double pos, len;
    static volatile boolean playing;
    private static long asked;
    private static boolean busy;

    static void draw(Simple m, Ctx c) {
        long now = System.currentTimeMillis();
        if (!busy && now - asked > 2000 && Api.available() && !c.preview) {
            busy = true; asked = now;
            CompletableFuture.runAsync(() -> {
                try {
                    JsonObject o = Api.call("GET", "/media", null);
                    title = o.has("title") ? o.get("title").getAsString() : "";
                    artist = o.has("artist") ? o.get("artist").getAsString() : "";
                    pos = o.has("pos") ? o.get("pos").getAsDouble() : 0;
                    len = o.has("len") ? o.get("len").getAsDouble() : 0;
                    playing = o.has("playing") && o.get("playing").getAsBoolean();
                } catch (Exception e) {
                    title = "";
                } finally {
                    busy = false;
                }
            });
        }
        String t = c.preview ? "Название трека" : title, a = c.preview ? "Исполнитель" : artist;
        if (t.isEmpty()) { m.size(0, 0); return; }
        int w = Math.max(100, Math.max(Gfx.tw(t), Gfx.tw(a)) + 40), h = 32;
        m.size(w, h);
        Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
        Gfx.rr(c.g, 5, 5, 22, 22, 6, Gfx.alpha(c.t.accent(), 0.35));
        Gfx.text(c.g, playing || c.preview ? "♪" : "II", 13, 12, c.t.accent(), false);
        Gfx.text(c.g, t.length() > 28 ? t.substring(0, 27) + "…" : t, 32, 6, c.t.text(), false);
        Gfx.text(c.g, a.length() > 30 ? a.substring(0, 29) + "…" : a, 32, 17, c.t.sub(), false);
        double v = c.preview ? 0.4 : (len > 0 ? pos / len : 0);
        Gfx.bar(c.g, 32, h - 5, w - 38, 2, v, c.t.accent(), Gfx.alpha(0x000000, 0.35));
    }
}
