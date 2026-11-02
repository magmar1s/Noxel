package noxel.mod.vis;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.screens.inventory.AbstractContainerScreen;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

/** Трекер хранилищ: запоминает содержимое открытых сундуков и ищет предметы по всему миру. */
public final class Storage {
    private Storage() {}

    public record Rec(String world, String dim, int x, int y, int z, Map<String, Integer> items, long at) {}

    private static final List<Rec> RECS = new ArrayList<>();
    private static boolean loaded, dirty;
    private static long lastSave;
    public static String search = "";
    private static BlockPos lastClicked;
    private static AbstractContainerScreen<?> watching;
    private static int lastHash;

    private static Path file() {
        return Cfg.path().resolveSibling("noxel-storage.json");
    }

    private static synchronized void load() {
        if (loaded) return;
        loaded = true;
        try {
            if (Files.exists(file())) {
                for (var e : JsonParser.parseString(Files.readString(file())).getAsJsonArray()) {
                    JsonObject o = e.getAsJsonObject();
                    Map<String, Integer> m = new LinkedHashMap<>();
                    for (var en : o.getAsJsonObject("i").entrySet()) m.put(en.getKey(), en.getValue().getAsInt());
                    RECS.add(new Rec(o.get("w").getAsString(), o.get("d").getAsString(), o.get("x").getAsInt(), o.get("y").getAsInt(), o.get("z").getAsInt(), m, o.get("t").getAsLong()));
                }
            }
        } catch (Exception ignored) {
        }
    }

    private static synchronized void save() {
        try {
            JsonArray a = new JsonArray();
            for (Rec r : RECS) {
                JsonObject o = new JsonObject(), it = new JsonObject();
                o.addProperty("w", r.world); o.addProperty("d", r.dim); o.addProperty("x", r.x); o.addProperty("y", r.y); o.addProperty("z", r.z); o.addProperty("t", r.at);
                for (var e : r.items.entrySet()) it.addProperty(e.getKey(), e.getValue());
                o.add("i", it);
                a.add(o);
            }
            Files.createDirectories(file().getParent());
            Files.writeString(file(), a.toString());
        } catch (Exception ignored) {
        }
        dirty = false;
        lastSave = System.currentTimeMillis();
    }

    public static synchronized List<Rec> all(Minecraft mc) {
        load();
        List<Rec> out = new ArrayList<>();
        for (Rec r : RECS) if (r.world.equals(Waypoints.key(mc))) out.add(r);
        return out;
    }

    public static synchronized void forget(Rec r) {
        RECS.remove(r);
        dirty = true;
    }

    /** Раз в тик: следим за открытым контейнером и обновляем запись. */
    public static void tick(Minecraft mc) {
        load();
        if (dirty && System.currentTimeMillis() - lastSave > 3000) save();
        var p = mc.player;
        if (p == null) return;
        if (mc.hitResult instanceof BlockHitResult bh && mc.options.keyUse.isDown() && mc.gui.screen() == null) lastClicked = bh.getBlockPos();
        if (!(mc.gui.screen() instanceof AbstractContainerScreen<?> sc) || lastClicked == null) { watching = null; return; }
        if (sc.getMenu().slots.size() < 10 || sc.getMenu().slots.get(0).container == p.getInventory()) return;
        Map<String, Integer> items = new LinkedHashMap<>();
        int h = 1;
        for (var s : sc.getMenu().slots) {
            if (s.container == p.getInventory()) continue;
            ItemStack st = s.getItem();
            if (st.isEmpty()) continue;
            String id = BuiltInRegistries.ITEM.getKey(st.getItem()).getPath();
            items.merge(id, st.getCount(), Integer::sum);
            h = h * 31 + id.hashCode() + st.getCount();
        }
        if (watching == sc && h == lastHash) return;
        watching = sc; lastHash = h;
        synchronized (Storage.class) {
            RECS.removeIf(r -> r.world.equals(Waypoints.key(mc)) && r.dim.equals(Waypoints.dim(mc)) && r.x == lastClicked.getX() && r.y == lastClicked.getY() && r.z == lastClicked.getZ());
            if (!items.isEmpty() || true) RECS.add(new Rec(Waypoints.key(mc), Waypoints.dim(mc), lastClicked.getX(), lastClicked.getY(), lastClicked.getZ(), items, System.currentTimeMillis()));
            dirty = true;
        }
    }

    public static boolean matches(Rec r, String q) {
        q = q.toLowerCase().trim().replace(' ', '_');
        if (q.isEmpty()) return true;
        for (String k : r.items.keySet()) if (k.contains(q)) return true;
        return false;
    }

    /** Подсветка сундуков с искомым предметом в мире. */
    static void marks(Ctx c, Module m) {
        if (search.isBlank()) return;
        Minecraft mc = c.mc;
        for (Rec r : all(mc)) {
            if (!r.dim.equals(Waypoints.dim(mc)) || !matches(r, search)) continue;
            double[] s = Proj.to(new Vec3(r.x + 0.5, r.y + 1.1, r.z + 0.5));
            if (s == null) continue;
            int n = 0;
            String q = search.toLowerCase().trim().replace(' ', '_');
            for (var e : r.items.entrySet()) if (e.getKey().contains(q)) n += e.getValue();
            String t = n + " шт";
            int w = Gfx.tw(t) + 8;
            int x = (int) s[0], y = (int) s[1];
            Gfx.rr(c.g, x - w / 2, y - 6, w, 13, 4, Gfx.alpha(c.t.bg(), 0.92));
            Gfx.ring(c.g, x - w / 2, y - 6, w, 13, 4, Gfx.alpha(c.t.accent(), 0.9));
            Gfx.text(c.g, t, x - w / 2 + 4, y - 3, c.t.accent(), false);
        }
    }
}
