package noxel.mod.vis;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;

/** Свои метки: хранятся в настройках визуалов отдельно для каждого сервера/мира. */
final class Waypoints {
    private Waypoints() {}

    static String key(Minecraft mc) {
        var sd = mc.getCurrentServer();
        return sd != null ? sd.ip : "local";
    }

    static String dim(Minecraft mc) {
        return mc.level == null ? "" : mc.level.dimension().toString();
    }

    static void add(Minecraft mc) {
        if (mc.player == null) return;
        JsonArray a = Cfg.arr("waypoints");
        int n = 1;
        for (var e : a) if (e.getAsJsonObject().get("k").getAsString().equals(key(mc))) n++;
        JsonObject o = new JsonObject();
        o.addProperty("k", key(mc));
        o.addProperty("d", dim(mc));
        o.addProperty("n", "Метка " + n);
        o.addProperty("x", (int) mc.player.getX());
        o.addProperty("y", (int) mc.player.getY());
        o.addProperty("z", (int) mc.player.getZ());
        a.add(o);
        Cfg.touch();
        Stats.notify("Поставил: Метка " + n, 0xFF5BE3A8);
    }

    static void clear(Minecraft mc) {
        JsonArray a = Cfg.arr("waypoints"), keep = new JsonArray();
        for (var e : a) if (!e.getAsJsonObject().get("k").getAsString().equals(key(mc))) keep.add(e);
        while (a.size() > 0) a.remove(0);
        for (var e : keep) a.add(e);
        Cfg.touch();
    }

    static void draw(Simple m, Ctx c) {
        Minecraft mc = c.mc;
        List<String[]> rows = new ArrayList<>();
        if (mc.player != null && mc.level != null) {
            record W(String n, double d, double ang) {}
            List<W> ws = new ArrayList<>();
            for (var e : Cfg.arr("waypoints")) {
                JsonObject o = e.getAsJsonObject();
                if (!o.get("k").getAsString().equals(key(mc)) || !o.get("d").getAsString().equals(dim(mc))) continue;
                double dx = o.get("x").getAsDouble() + 0.5 - mc.player.getX(), dz = o.get("z").getAsDouble() + 0.5 - mc.player.getZ();
                double target = Math.toDegrees(Math.atan2(-dx, dz)), rel = (((target - mc.player.getYRot()) % 360) + 540) % 360 - 180;
                ws.add(new W(o.get("n").getAsString(), Math.hypot(dx, dz), rel));
            }
            ws.sort((a, b) -> Double.compare(a.d, b.d));
            String[] arrows = {"↑", "↗", "→", "↘", "↓", "↙", "←", "↖"};
            for (int i = 0; i < Math.min((int) m.n("limit"), ws.size()); i++) {
                W w = ws.get(i);
                int ai = (int) Math.floor(((w.ang % 360 + 360) % 360 + 22.5) / 45) % 8;
                rows.add(Simple.r(w.n, arrows[ai] + " " + (int) w.d + " м"));
            }
        } else {
            rows.add(Simple.r("Дом", "↗ 120 м")); rows.add(Simple.r("Шахта", "← 340 м"));
        }
        if (rows.isEmpty()) { m.size(0, 0); return; }
        m.listOut(c, "Метки", rows);
    }
}
