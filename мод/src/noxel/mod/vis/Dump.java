package noxel.mod.vis;

import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;

/** Печатает каталог модулей в JSON: из него лаунчер строит страницу «Визуалы». Запуск: build.sh. */
public final class Dump {
    public static void main(String[] a) {
        JsonObject root = new JsonObject();
        JsonArray mods = new JsonArray();
        for (Module m : Mods.ALL) {
            JsonObject o = new JsonObject();
            o.addProperty("id", m.id); o.addProperty("name", m.name); o.addProperty("cat", m.cat); o.addProperty("desc", m.desc);
            o.addProperty("on", m.defOn); o.addProperty("panel", m.panel);
            JsonArray opts = new JsonArray();
            for (Opt p : m.opts) {
                JsonObject q = new JsonObject();
                q.addProperty("key", p.key); q.addProperty("name", p.name); q.addProperty("kind", p.kind.name().toLowerCase());
                q.addProperty("min", p.min); q.addProperty("max", p.max); q.addProperty("step", p.step); q.addProperty("def", p.def);
                if (p.choices != null) { JsonArray c = new JsonArray(); for (String s : p.choices) c.add(s); q.add("choices", c); }
                opts.add(q);
            }
            o.add("opts", opts);
            mods.add(o);
        }
        root.add("modules", mods);
        JsonObject pr = new JsonObject();
        for (var e : Presets.ALL.entrySet()) {
            JsonObject p = new JsonObject();
            p.addProperty("desc", Presets.DESC.get(e.getKey()));
            JsonArray ids = new JsonArray();
            for (String id : e.getValue()) ids.add(id);
            p.add("ids", ids);
            pr.add(e.getKey(), p);
        }
        root.add("presets", pr);
        JsonObject st = new JsonObject();
        JsonArray styles = new JsonArray(), accents = new JsonArray();
        for (String s : Theme.STYLES) styles.add(s);
        for (String s : Theme.ACCENTS) accents.add(s);
        st.add("styles", styles); st.add("accents", accents);
        root.add("themes", st);
        System.out.println(new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(root));
    }
}
