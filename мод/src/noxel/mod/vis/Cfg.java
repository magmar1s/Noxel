package noxel.mod.vis;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.fabricmc.loader.api.FabricLoader;

/** Настройки визуалов: один JSON-файл. Лаунчер Noxel правит тот же файл, мод перечитывает его при изменении. */
public final class Cfg {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static Path file;
    private static JsonObject root = new JsonObject();
    private static long stamp, lastCheck, lastWrite;
    private static boolean dirty;

    private Cfg() {}

    public static Path path() {
        if (file == null) {
            String p = System.getProperty("noxel.visuals");
            file = p != null ? Path.of(p) : FabricLoader.getInstance().getConfigDir().resolve("noxel-visuals.json");
        }
        return file;
    }

    public static synchronized void load() {
        try {
            Path p = path();
            if (Files.exists(p)) {
                root = JsonParser.parseString(Files.readString(p, StandardCharsets.UTF_8)).getAsJsonObject();
                stamp = Files.getLastModifiedTime(p).toMillis();
            }
        } catch (Exception e) {
            root = new JsonObject();
        }
    }

    /** Вызывается каждый кадр: раз в секунду смотрит, не изменил ли файл лаунчер; раз в 2 секунды сохраняет свои правки. */
    public static synchronized void poll() {
        long now = System.currentTimeMillis();
        if (now - lastCheck < 1000) return;
        lastCheck = now;
        try {
            if (dirty && now - lastWrite > 1500) save();
            Path p = path();
            if (!dirty && Files.exists(p) && Files.getLastModifiedTime(p).toMillis() != stamp) load();
        } catch (Exception ignored) {
        }
    }

    public static synchronized void save() {
        try {
            Path p = path();
            Files.createDirectories(p.getParent());
            Files.writeString(p, GSON.toJson(root), StandardCharsets.UTF_8);
            stamp = Files.getLastModifiedTime(p).toMillis();
        } catch (Exception ignored) {
        }
        dirty = false;
        lastWrite = System.currentTimeMillis();
    }

    private static JsonObject obj(String name) {
        if (!root.has(name) || !root.get(name).isJsonObject()) root.add(name, new JsonObject());
        return root.getAsJsonObject(name);
    }

    private static JsonObject mod(String id) {
        JsonObject m = obj("modules");
        if (!m.has(id) || !m.get(id).isJsonObject()) m.add(id, new JsonObject());
        return m.getAsJsonObject(id);
    }

    // ── общие настройки ──
    public static String gs(String k, String d) {
        JsonElement e = obj("style").get(k);
        return e != null && e.isJsonPrimitive() ? e.getAsString() : d;
    }

    public static double gn(String k, double d) {
        JsonElement e = obj("style").get(k);
        try {
            return e != null && e.isJsonPrimitive() ? e.getAsDouble() : d;
        } catch (Exception x) {
            return d;
        }
    }

    public static synchronized void setStyle(String k, String v) {
        obj("style").addProperty(k, v);
        dirty = true;
    }

    public static synchronized void setStyle(String k, double v) {
        obj("style").addProperty(k, v);
        dirty = true;
    }

    // ── модули ──
    public static boolean enabled(String id, boolean d) {
        JsonElement e = mod(id).get("on");
        return e != null && e.isJsonPrimitive() ? e.getAsBoolean() : d;
    }

    public static synchronized void setEnabled(String id, boolean v) {
        mod(id).addProperty("on", v);
        dirty = true;
    }

    public static double num(String id, String k, double d) {
        JsonElement e = mod(id).get(k);
        try {
            return e != null && e.isJsonPrimitive() ? e.getAsDouble() : d;
        } catch (Exception x) {
            return d;
        }
    }

    public static synchronized void setNum(String id, String k, double v) {
        mod(id).addProperty(k, v);
        dirty = true;
    }

    private static JsonObject undo;

    /** Запоминает модули и стиль перед большим действием (набор, сброс): потом можно вернуть. */
    public static synchronized void snapshotUndo() {
        undo = new JsonObject();
        for (String k : new String[]{"modules", "style"}) if (root.has(k)) undo.add(k, root.get(k).deepCopy());
    }

    public static synchronized boolean undo() {
        if (undo == null) return false;
        for (String k : new String[]{"modules", "style"}) { root.remove(k); if (undo.has(k)) root.add(k, undo.get(k)); }
        undo = null;
        dirty = true;
        return true;
    }

    public static synchronized void markSeen() {
        mod("ui").addProperty("seen", 1);
        dirty = true;
    }

    public static boolean has(String id, String k) {
        return mod(id).has(k);
    }

    public static synchronized void unset(String id, String k) {
        mod(id).remove(k);
        dirty = true;
    }

    public static synchronized void reset(String id) {
        obj("modules").remove(id);
        dirty = true;
    }

    public static synchronized com.google.gson.JsonArray arr(String name) {
        if (!root.has(name) || !root.get(name).isJsonArray()) root.add(name, new com.google.gson.JsonArray());
        return root.getAsJsonArray(name);
    }

    public static synchronized void touch() {
        dirty = true;
    }

    // ── обмен настройками: строка «NXV1:…» (base64) ──
    public static synchronized String export() {
        return "NXV1:" + java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(share().toString().getBytes(StandardCharsets.UTF_8));
    }

    private static JsonObject share() {
        JsonObject o = new JsonObject();
        for (String k : new String[]{"modules", "style"}) if (root.has(k)) o.add(k, root.get(k));
        return o;
    }

    public static synchronized boolean importCode(String code) {
        try {
            code = code.trim();
            if (!code.startsWith("NXV1:")) return false;
            JsonObject o = JsonParser.parseString(new String(java.util.Base64.getUrlDecoder().decode(code.substring(5)), StandardCharsets.UTF_8)).getAsJsonObject();
            for (String k : new String[]{"modules", "style"}) if (o.has(k)) root.add(k, o.get(k));   // метки и память о сундуках у каждого свои
            dirty = true;
            save();
            return true;
        } catch (Exception e) {
            return false;
        }
    }

    public static synchronized void replaceAll(JsonObject o) {
        root = o;
        dirty = true;
        save();
    }

    public static synchronized JsonObject snapshot() {
        return root.deepCopy();
    }
}
