package noxel.mod.vis;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Готовые наборы: включают нужные модули и выключают остальные. */
public final class Presets {
    private Presets() {}

    public static final Map<String, List<String>> ALL = new LinkedHashMap<>();
    public static final Map<String, String> DESC = new LinkedHashMap<>();

    static {
        add("Минимум", "Только остров и FPS: ничего лишнего", "island", "perf");
        add("Классика", "Базовый набор: информация, эффекты, броня, клавиши", "island", "perf", "coords", "potions", "armor", "keys", "death", "vignette");
        add("PvP", "Всё для боя: цель, запасы, CPS, игроки рядом, предсказание бросков", "island", "perf", "potions", "armor", "items", "keys", "cps", "near", "target", "combo", "predict", "tags", "radar", "vignette", "tntworld", "crosshair", "hotbar");
        add("Выживание", "Метки, координаты, трекер сундуков, радар и точка смерти", "island", "coords", "compass", "clock", "waypoints", "wpworld", "storage", "death", "radar", "potions", "armor", "zoom");
        add("Стример", "Красиво и без лишнего: остров, клавиши, CPS, плеер, стеклянный хотбар", "island", "keys", "cps", "media", "hotbar", "crosshair", "zoom", "chattime");
        add("Всё сразу", "Все модули, кроме помощников (они только вручную)", Mods.ALL.stream().filter(m -> !m.cat.equals(Mods.ASSIST)).map(m -> m.id).toArray(String[]::new));
    }

    private static void add(String name, String desc, String... ids) {
        ALL.put(name, List.of(ids));
        DESC.put(name, desc);
    }

    public static void apply(String name) {
        List<String> on = ALL.get(name);
        if (on == null) return;
        for (Module m : Mods.ALL) Cfg.setEnabled(m.id, on.contains(m.id));
    }
}
