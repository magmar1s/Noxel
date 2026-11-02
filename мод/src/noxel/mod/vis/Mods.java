package noxel.mod.vis;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;

/** Реестр всех модулей. Порядок здесь = порядок в меню. */
public final class Mods {
    private Mods() {}

    public static final String INFO = "Информация", COMBAT = "Бой", WORLD = "Мир", SCREEN = "Экран", MISC = "Прочее", ASSIST = "Помощники";
    public static final List<Module> ALL = new ArrayList<>();

    static Simple add(Simple m) { return reg(m); }

    private static Simple reg(Simple m) {
        ALL.add(m);
        return m;
    }

    public static Module get(String id) {
        for (Module m : ALL) if (m.id.equals(id)) return m;
        return null;
    }

    private static String mmss(int ticks) {
        int s = Math.max(0, ticks / 20);
        return s >= 3600 ? (s / 3600) + ":" + String.format("%02d:%02d", s / 60 % 60, s % 60) : String.format("%d:%02d", s / 60, s % 60);
    }

    private static final String[] ROMAN = {"", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"};
    private static final String[] DIRS = {"Ю", "ЮЗ", "З", "СЗ", "С", "СВ", "В", "ЮВ"};

    static String dir(float yaw) {
        int i = (int) Math.floor(((yaw % 360 + 360) % 360 + 22.5) / 45) % 8;
        return DIRS[i];
    }

    static {
        // ───────────── Информация ─────────────
        reg(new Simple("island", "Остров", INFO, "Верхняя «таблетка»: время, FPS, пинг и всплывающие уведомления", true, 0.5, 0.01, (m, c) -> {
            Minecraft mc = c.mc;
            boolean pop = System.currentTimeMillis() - Stats.toastAt < 3200 && !Stats.toast.isEmpty();
            String base = "Noxel  " + java.time.LocalTime.now().format(java.time.format.DateTimeFormatter.ofPattern("HH:mm"));
            if (m.b("fps")) base += "  " + Stats.fps + " fps";
            if (m.b("ping")) base += "  " + ping(mc) + " мс";
            String txt = pop ? Stats.toast : base;
            m.pillOut(c, txt, pop ? Stats.toastColor : c.t.accent());
        }).std().add(Opt.bool("fps", "Показывать FPS", true)).add(Opt.bool("ping", "Показывать пинг", true)));

        reg(new Simple("watermark", "Ватермарк", INFO, "Название и ник в углу", false, 0.01, 0.01, (m, c) -> {
            String nick = c.mc.getUser().getName();
            m.pillOut(c, "Noxel · " + nick, c.t.accent());
        }).std());

        reg(new Simple("perf", "Производительность", INFO, "FPS, график кадров, память и пинг", true, 0.01, 0.12, (m, c) -> {
            Minecraft mc = c.mc;
            List<String[]> rows = new ArrayList<>();
            rows.add(Simple.r("FPS", Stats.fps + (m.b("avg") ? "  (ср. " + (int) Stats.fpsAvg + ")" : ""), Stats.fps < 30 ? 0xFFFF6B6B : Stats.fps < 60 ? 0xFFFFC14D : c.t.text()));
            if (m.b("ping")) rows.add(Simple.r("Пинг", ping(mc) + " мс", ping(mc) > 150 ? 0xFFFF6B6B : c.t.text()));
            Runtime rt = Runtime.getRuntime();
            long used = (rt.totalMemory() - rt.freeMemory()) >> 20, max = rt.maxMemory() >> 20;
            if (m.b("mem")) rows.add(Simple.r("Память", used + " / " + max + " МБ"));
            m.listOut(c, "Производительность", rows);
            if (m.b("graph")) {
                int gh = 14, gy = m.h + 2;
                m.size(m.w, m.h + gh + 4);
                Gfx.glass(c.g, c.t, 0, gy - 2, m.w, gh + 4, m.alpha());
                int n = Math.min(Stats.FRAMES.length, (m.w - 8) / 2);
                for (int i = 0; i < n; i++) {
                    float ms = Stats.FRAMES[(Stats.frameIdx + Stats.FRAMES.length - n + i) % Stats.FRAMES.length];
                    int bh = (int) Math.min(gh, Math.max(1, ms / 33f * gh));
                    int col = ms > 33 ? 0xFFFF6B6B : ms > 20 ? 0xFFFFC14D : c.t.accent();
                    c.g.fill(4 + i * 2, gy + gh - bh, 4 + i * 2 + 1, gy + gh, Gfx.alpha(col, 0.9));
                }
            }
        }).std().add(Opt.bool("avg", "Средний FPS", true)).add(Opt.bool("ping", "Пинг", true)).add(Opt.bool("mem", "Память", true)).add(Opt.bool("graph", "График кадров", true)));

        reg(new Simple("coords", "Координаты", INFO, "XYZ, направление, биом, пересчёт для Незера", true, 0.01, 0.34, (m, c) -> {
            var p = c.mc.player;
            double x = p == null ? 128.5 : p.getX(), y = p == null ? 72 : p.getY(), z = p == null ? -40.2 : p.getZ();
            List<String[]> rows = new ArrayList<>();
            rows.add(Simple.r("XYZ", String.format("%.0f %.0f %.0f", x, y, z)));
            if (m.b("dir")) rows.add(Simple.r("Курс", p == null ? "СВ" : dir(p.getYRot())));
            if (m.b("biome") && p != null && c.mc.level != null) rows.add(Simple.r("Биом", biome(c.mc)));
            if (m.b("nether") && c.mc.level != null && c.mc.level.dimension().toString().contains("the_nether")) rows.add(Simple.r("Верхний мир", String.format("%.0f %.0f", x * 8, z * 8)));
            else if (m.b("nether") && c.mc.level != null && c.mc.level.dimension().toString().contains("overworld")) rows.add(Simple.r("Незер", String.format("%.0f %.0f", x / 8, z / 8)));
            m.listOut(c, "Координаты", rows);
        }).std().add(Opt.bool("dir", "Направление", true)).add(Opt.bool("biome", "Биом", true)).add(Opt.bool("nether", "Пересчёт Незера", true)));

        reg(new Simple("compass", "Компас", INFO, "Лента направлений как в шутерах", false, 0.5, 0.06, (m, c) -> {
            float yaw = c.mc.player == null ? 30 : c.mc.player.getYRot();
            int w = 150, h = 15;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            String[] names = {"Ю", "ЮЗ", "З", "СЗ", "С", "СВ", "В", "ЮВ"};
            for (int i = -8; i <= 16; i++) {
                double ang = i * 22.5;
                double off = (((ang - yaw) % 360) + 540) % 360 - 180;
                int px = (int) (w / 2 + off / 90 * (w / 2));
                if (px < 6 || px > w - 6) continue;
                boolean major = ((i % 2) + 2) % 2 == 0;
                String s = major ? names[((i / 2) % 4 + 4) % 4 * 2 % 8] : "";
                if (major) Gfx.ctext(c.g, s, px, 3, Math.abs(off) < 12 ? c.t.accent() : Gfx.alpha(c.t.text(), 0.75), false);
                else c.g.fill(px, 5, px + 1, 10, Gfx.alpha(c.t.sub(), 0.6));
            }
            c.g.fill(w / 2, h - 4, w / 2 + 1, h - 1, c.t.accent());
        }).std());

        reg(new Simple("clock", "Часы", INFO, "Реальное время, время суток в игре и время сессии", false, 0.01, 0.5, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            rows.add(Simple.r("Время", java.time.LocalTime.now().format(java.time.format.DateTimeFormatter.ofPattern(m.b("sec") ? "HH:mm:ss" : "HH:mm"))));
            if (c.mc.level != null) {
                long t = c.mc.level.getDefaultClockTime() % 24000;
                int hh = (int) ((t / 1000 + 6) % 24), mm = (int) (t % 1000 * 60 / 1000);
                rows.add(Simple.r("В игре", String.format("%02d:%02d", hh, mm)));
            }
            long s = (System.currentTimeMillis() - Stats.START) / 1000;
            if (m.b("session")) rows.add(Simple.r("Сессия", s >= 3600 ? String.format("%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60) : String.format("%d:%02d", s / 60, s % 60)));
            m.listOut(c, "Часы", rows);
        }).std().add(Opt.bool("sec", "Секунды", false)).add(Opt.bool("session", "Время сессии", true)));

        reg(new Simple("speed", "Скорость", INFO, "Скорость движения в блоках в секунду", false, 0.01, 0.62, (m, c) -> m.pillOut(c, String.format("%.1f бл/с", c.preview ? 4.3 : Stats.speed), c.t.accent())).std());

        reg(new Simple("server", "Сервер", INFO, "Адрес сервера, пинг и число игроков", false, 0.01, 0.68, (m, c) -> {
            Minecraft mc = c.mc;
            var sd = mc.getCurrentServer();
            List<String[]> rows = new ArrayList<>();
            rows.add(Simple.r("Адрес", sd != null ? sd.ip : mc.isLocalServer() ? "одиночная игра" : "—"));
            rows.add(Simple.r("Пинг", ping(mc) + " мс"));
            if (mc.getConnection() != null) rows.add(Simple.r("Игроки", String.valueOf(mc.getConnection().getOnlinePlayers().size())));
            m.listOut(c, "Сервер", rows);
        }).std());

        reg(new Simple("potions", "Эффекты", INFO, "Активные эффекты с таймерами", true, 0.99, 0.3, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            var p = c.mc.player;
            if (p != null) {
                for (MobEffectInstance e : p.getActiveEffects()) {
                    int amp = e.getAmplifier();
                    String nm = e.getEffect().value().getDisplayName().getString() + (amp > 0 && amp < ROMAN.length ? " " + ROMAN[amp + 1] : amp > 0 ? " " + (amp + 1) : "");
                    int left = e.getDuration();
                    rows.add(Simple.r(nm, e.isInfiniteDuration() ? "∞" : mmss(left), left < 200 && !e.isInfiniteDuration() ? 0xFFFF6B6B : c.t.text()));
                }
            } else {
                rows.add(Simple.r("Скорость II", "3:12"));
                rows.add(Simple.r("Сила", "1:05"));
            }
            if (rows.isEmpty()) { if (!m.b("empty")) { m.size(0, 0); return; } rows.add(Simple.r("нет эффектов", "")); }
            m.listOut(c, "Эффекты", rows);
        }).std().add(Opt.bool("empty", "Показывать пустым", false)));

        reg(new Simple("armor", "Броня", INFO, "Предметы на тебе и их прочность", true, 0.5, 0.88, (m, c) -> {
            var p = c.mc.player;
            EquipmentSlot[] sl = {EquipmentSlot.HEAD, EquipmentSlot.CHEST, EquipmentSlot.LEGS, EquipmentSlot.FEET};
            int cw = 24, w = 4 * cw + 6, h = m.b("pct") ? 36 : 28;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            for (int i = 0; i < 4; i++) {
                ItemStack st = p == null ? new ItemStack(i == 0 ? Items.DIAMOND_HELMET : i == 1 ? Items.DIAMOND_CHESTPLATE : i == 2 ? Items.DIAMOND_LEGGINGS : Items.DIAMOND_BOOTS) : p.getItemBySlot(sl[i]);
                int x = 3 + i * cw + 4;
                if (st.isEmpty()) { Gfx.rr(c.g, x + 3, 7, 10, 10, 3, Gfx.alpha(c.t.sub(), 0.25)); continue; }
                c.g.item(st, x, 4);
                if (st.isDamageableItem()) {
                    double v = 1 - st.getDamageValue() / (double) st.getMaxDamage();
                    int col = v < 0.2 ? 0xFFFF6B6B : v < 0.5 ? 0xFFFFC14D : 0xFF5BE3A8;
                    Gfx.bar(c.g, x, 22, 16, 2, v, col, Gfx.alpha(0x000000, 0.4));
                    if (m.b("pct")) Gfx.ctext(c.g, (int) Math.round(v * 100) + "%", x + 8, 26, Gfx.alpha(col, 1), false);
                }
            }
        }).std().add(Opt.bool("pct", "Проценты", false)));

        reg(new Simple("items", "Запасы", COMBAT, "Тотемы, жемчуг, яблоки, зелья и стрелы в инвентаре", true, 0.99, 0.62, (m, c) -> {
            var p = c.mc.player;
            Object[][] def = {
                {Items.TOTEM_OF_UNDYING, "totem"}, {Items.ENDER_PEARL, "pearl"}, {Items.GOLDEN_APPLE, "gapple"}, {Items.ENCHANTED_GOLDEN_APPLE, "egap"},
                {Items.EXPERIENCE_BOTTLE, "xp"}, {Items.ARROW, "arrow"}, {Items.TNT, "tnt"}, {Items.COBWEB, "web"}};
            List<Object[]> shown = new ArrayList<>();
            for (Object[] d : def) {
                if (!m.b((String) d[1])) continue;
                ItemStack icon = new ItemStack((net.minecraft.world.item.Item) d[0]);
                int n = 0;
                if (p != null) for (int i = 0; i < p.getInventory().getContainerSize(); i++) { ItemStack s = p.getInventory().getItem(i); if (s.is((net.minecraft.world.item.Item) d[0])) n += s.getCount(); }
                else n = 3;
                if (n > 0 || m.b("zero") || c.preview) shown.add(new Object[]{icon, n});
            }
            if (shown.isEmpty()) { m.size(0, 0); return; }
            int w = 8, rowH = 19, h = shown.size() * rowH + 6;
            for (Object[] s : shown) w = Math.max(w, 28 + Gfx.tw("x" + s[1]));
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            int y = 3;
            for (Object[] s : shown) {
                c.g.item((ItemStack) s[0], 4, y);
                int n = (Integer) s[1];
                Gfx.text(c.g, "x" + n, 24, y + 4, n == 0 ? Gfx.alpha(c.t.sub(), 0.6) : n <= 1 && s[0] != null && ((ItemStack) s[0]).is(Items.TOTEM_OF_UNDYING) ? 0xFFFFC14D : c.t.text(), false);
                y += rowH;
            }
        }).std().add(Opt.bool("totem", "Тотемы", true)).add(Opt.bool("pearl", "Жемчуг", true)).add(Opt.bool("gapple", "Золотые яблоки", true)).add(Opt.bool("egap", "Зачарованные яблоки", false))
            .add(Opt.bool("xp", "Пузырьки опыта", false)).add(Opt.bool("arrow", "Стрелы", false)).add(Opt.bool("tnt", "Динамит", false)).add(Opt.bool("web", "Паутина", false)).add(Opt.bool("zero", "Показывать нули", false)));

        reg(new Simple("inventory", "Инвентарь", INFO, "Весь инвентарь мини-сеткой на экране", false, 0.99, 0.82, (m, c) -> {
            var p = c.mc.player;
            int cols = 9, rows = 3, cell = 18, w = cols * cell + 8, h = rows * cell + 8;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            for (int i = 0; i < 27; i++) {
                int x = 4 + (i % 9) * cell, y = 4 + (i / 9) * cell;
                Gfx.rr(c.g, x, y, cell - 1, cell - 1, 3, Gfx.alpha(0x000000, 0.22 * m.alpha()));
                if (p == null) continue;
                ItemStack st = p.getInventory().getItem(9 + i);
                if (st.isEmpty()) continue;
                c.g.item(st, x, y);
                c.g.itemDecorations(c.mc.font, st, x, y);
            }
        }).std());

        reg(new Simple("keys", "Клавиши", INFO, "WASD, прыжок и мышь с подсветкой нажатий", true, 0.01, 0.8, (m, c) -> {
            var o = c.mc.options;
            int k = 17, g = 2, w = 3 * k + 2 * g, h = 3 * k + 2 * g + (m.b("mouse") ? 0 : -k - g);
            m.size(w, h);
            long ph = System.currentTimeMillis() / 650 % 6;   // в превью клавиши нажимаются сами
            boolean pv = c.preview;
            keyBox(c, m, "W", pv ? ph == 0 : o.keyUp.isDown(), k + g, 0, k, k);
            keyBox(c, m, "A", pv ? ph == 1 : o.keyLeft.isDown(), 0, k + g, k, k);
            keyBox(c, m, "S", pv ? ph == 2 : o.keyDown.isDown(), k + g, k + g, k, k);
            keyBox(c, m, "D", pv ? ph == 3 : o.keyRight.isDown(), 2 * (k + g), k + g, k, k);
            if (m.b("mouse")) {
                keyBox(c, m, "ЛКМ " + (m.b("cps") ? Stats.cps(Stats.LEFT) : ""), pv ? ph == 4 : o.keyAttack.isDown(), 0, 2 * (k + g), (w - g) / 2, k);
                keyBox(c, m, "ПКМ " + (m.b("cps") ? Stats.cps(Stats.RIGHT) : ""), pv ? ph == 5 : o.keyUse.isDown(), (w - g) / 2 + g, 2 * (k + g), (w - g) / 2, k);
            }
        }).std().add(Opt.bool("mouse", "Кнопки мыши", true)).add(Opt.bool("cps", "CPS на кнопках", true)));

        reg(new Simple("cps", "CPS", COMBAT, "Клики в секунду левой и правой кнопкой", false, 0.01, 0.74, (m, c) -> m.pillOut(c, "CPS  " + (c.preview ? 7 + System.currentTimeMillis() / 250 % 4 : Stats.cps(Stats.LEFT)) + " | " + (c.preview ? 2 + System.currentTimeMillis() / 400 % 3 : Stats.cps(Stats.RIGHT)), c.t.accent())).std());

        reg(new Simple("near", "Игроки рядом", COMBAT, "Кто рядом, на каком расстоянии и сколько у него здоровья", true, 0.99, 0.05, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            if (c.mc.level != null && c.mc.player != null) {
                List<Player> ps = new ArrayList<>(c.mc.level.players());
                ps.remove(c.mc.player);
                ps.removeIf(p -> p.distanceTo(c.mc.player) > m.n("range"));
                ps.sort((a, b) -> Float.compare(a.distanceTo(c.mc.player), b.distanceTo(c.mc.player)));
                int lim = (int) m.n("limit");
                for (int i = 0; i < Math.min(lim, ps.size()); i++) {
                    Player p = ps.get(i);
                    float d = p.distanceTo(c.mc.player);
                    String hp = m.b("hp") ? "  " + (int) Math.ceil(p.getHealth()) + "♥" : "";
                    rows.add(Simple.r(p.getName().getString(), (int) d + " м" + hp, d < 6 ? 0xFFFF6B6B : d < 20 ? 0xFFFFC14D : c.t.text()));
                }
            } else {
                rows.add(Simple.r("Kaktus", "7 м  20♥")); rows.add(Simple.r("Frostbyte", "14 м  18♥"));
            }
            if (rows.isEmpty()) { m.size(0, 0); return; }
            m.listOut(c, "Игроки рядом", rows);
        }).std().add(Opt.num("range", "Дальность", 8, 128, 4, 64)).add(Opt.num("limit", "Сколько показывать", 1, 10, 1, 5)).add(Opt.bool("hp", "Здоровье", true)));

        reg(new Simple("target", "Цель", COMBAT, "Панель противника: здоровье, броня, расстояние", true, 0.5, 0.62, (m, c) -> {
            LivingEntity t = Stats.target;
            String name = t == null ? "Kaktus" : t.getName().getString();
            float hp = t == null ? 15 : t.getHealth(), max = t == null ? 20 : t.getMaxHealth();
            if (t == null && !c.preview) { m.size(0, 0); return; }
            int armor = t == null ? 14 : t.getArmorValue();
            double dist = t == null || c.mc.player == null ? 3.4 : t.distanceTo(c.mc.player);
            int w = 118, h = 34;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            Gfx.text(c.g, name, 7, 5, c.t.text(), false);
            String right = String.format("%.1f м", dist);
            Gfx.text(c.g, right, w - 7 - Gfx.tw(right), 5, Gfx.alpha(c.t.sub(), 1), false);
            double v = max <= 0 ? 0 : hp / max;
            Gfx.bar(c.g, 7, 17, w - 14, 5, v, v < 0.3 ? 0xFFFF6B6B : v < 0.6 ? 0xFFFFC14D : 0xFF5BE3A8, Gfx.alpha(0x000000, 0.35));
            Gfx.text(c.g, String.format("%.1f / %.0f", hp, max) + (m.b("armor") ? "   ⛨ " + armor : ""), 7, 24, Gfx.alpha(c.t.sub(), 1), false);
        }).std().add(Opt.bool("armor", "Показывать броню", true)));

        reg(new Simple("combo", "Комбо", COMBAT, "Серия ударов без получения урона", false, 0.5, 0.45, (m, c) -> {
            if (Stats.combo < 2 && !c.preview) { m.size(0, 0); return; }
            m.pillOut(c, "Комбо  x" + (c.preview ? 7 : Stats.combo) + (m.b("best") ? "   лучшее " + Math.max(Stats.bestCombo, c.preview ? 12 : 0) : ""), 0xFFFFC14D);
        }).std().add(Opt.bool("best", "Показывать лучшее", false)));

        reg(new Simple("tnt", "Таймер TNT", WORLD, "Сколько осталось до взрыва ближайшего динамита", true, 0.5, 0.25, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            if (c.mc.level != null && c.mc.player != null) {
                List<PrimedTnt> l = new ArrayList<>();
                for (var e : c.mc.level.entitiesForRendering()) if (e instanceof PrimedTnt t && t.distanceTo(c.mc.player) < 64) l.add(t);
                l.sort((a, b) -> Float.compare(a.distanceTo(c.mc.player), b.distanceTo(c.mc.player)));
                for (int i = 0; i < Math.min(3, l.size()); i++) {
                    PrimedTnt t = l.get(i);
                    double s = t.getFuse() / 20.0;
                    rows.add(Simple.r(String.format("%.1f с", s), (int) t.distanceTo(c.mc.player) + " м", s < 1.5 ? 0xFFFF6B6B : c.t.text()));
                }
            } else rows.add(Simple.r("2.8 с", "5 м"));
            if (rows.isEmpty()) { m.size(0, 0); return; }
            m.listOut(c, "TNT", rows);
        }).std());

        reg(new Simple("death", "Точка смерти", WORLD, "Где ты в последний раз умер", true, 0.01, 0.9, (m, c) -> {
            if (Stats.death.isEmpty() && !c.preview) { m.size(0, 0); return; }
            m.pillOut(c, "Смерть: " + (c.preview ? "120 64 -33" : Stats.death), 0xFFFF6B6B);
        }).std());

        reg(new Simple("radar", "Радар", WORLD, "Круглая карта вокруг тебя: игроки, мобы, предметы и рельеф", true, 0.99, 0.5, (m, c) -> Radar.draw(m, c)).std()
            .add(Opt.num("size", "Диаметр", 60, 140, 10, 90)).add(Opt.num("range", "Дальность (блоки)", 16, 96, 8, 40)).add(Opt.bool("terrain", "Рельеф", true))
            .add(Opt.bool("players", "Игроки", true)).add(Opt.bool("mobs", "Мобы", true)).add(Opt.bool("items", "Предметы", false)).add(Opt.bool("rotate", "Вращать по взгляду", true)));

        reg(new Simple("waypoints", "Метки", WORLD, "Свои метки с расстоянием и стрелкой (клавиша B ставит метку)", true, 0.01, 0.22, (m, c) -> Waypoints.draw(m, c)).std().add(Opt.num("limit", "Сколько показывать", 1, 8, 1, 4)));


        // ───────────── Мир: подписи и траектории прямо в мире ─────────────
        reg(new Simple("tntworld", "TNT в мире", WORLD, "Секунды до взрыва прямо над динамитом", true, 0, 0, (m, c) -> {}).noPanel());
        reg(new Simple("wpworld", "Метки в мире", WORLD, "Твои метки видны сквозь мир с расстоянием", true, 0, 0, (m, c) -> {}).noPanel());
        reg(new Simple("predict", "Предсказание бросков", WORLD, "Траектория и точка падения жемчуга, зелий, лука, трезубца и летящих снарядов", true, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.bool("hand", "Предмет в руке", true)).add(Opt.bool("flying", "Летящие снаряды", true)).add(Opt.bool("time", "Время полёта", true)));
        reg(new Simple("tags", "Здоровье над игроками", COMBAT, "Полоска здоровья и брони над другими игроками", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.num("range", "Дальность", 8, 64, 4, 32)).add(Opt.bool("armor", "Броня", true)));
        reg(new Simple("storage", "Трекер хранилищ", WORLD, "Помнит содержимое каждого открытого сундука; поиск по предмету и подсветка в мире (меню → Хранилище)", true, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.bool("world", "Подсвечивать в мире", true)));
        reg(new Simple("zoom", "Приближение", SCREEN, "Зажми клавишу C, чтобы приблизить картинку; колесо меняет силу", true, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.num("fov", "Угол при зуме", 10, 60, 5, 25)).add(Opt.bool("smooth", "Плавно", true)));
        reg(new Simple("media", "Плеер", INFO, "Что играет сейчас на компьютере: название, исполнитель, прогресс (читает лаунчер Noxel)", false, 0.01, 0.58, (m, c) -> Media.draw(m, c)).std());

        // ───────────── Экран ─────────────
        reg(new Simple("crosshair", "Прицел", SCREEN, "Свой прицел: форма, цвет, размер, зазор, реакция на движение", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.choice("shape", "Форма", 1, "Точка", "Крест", "Круг", "Квадрат")).add(Opt.num("size", "Размер", 2, 14, 1, 5)).add(Opt.num("gap", "Зазор", 0, 8, 1, 2)).add(Opt.num("thick", "Толщина", 1, 3, 1, 1))
            .add(Opt.choice("color", "Цвет", 0, "Акцент", "Белый", "Красный", "Зелёный", "Радуга")).add(Opt.bool("dyn", "Расходится при беге", true)).add(Opt.bool("outline", "Обводка", true)));
        reg(new Simple("hotbar", "Стеклянный хотбар", SCREEN, "Заменяет панель предметов на стеклянную", false, 0, 0, (m, c) -> {}).noPanel());
        reg(new Simple("vignette", "Предупреждение о здоровье", SCREEN, "Края экрана краснеют, когда мало здоровья", true, 0, 0, (m, c) -> {}).noPanel().add(Opt.num("hp", "Порог (сердец)", 1, 10, 1, 4)));
        reg(new Simple("chattime", "Время в чате", MISC, "Добавляет время к каждому сообщению в чате", false, 0, 0, (m, c) -> {}).noPanel());
        More.load();
        Assist.load();
        ViewModel.load();
    }

    static void keyBox(Ctx c, Module m, String s, boolean down, int x, int y, int w, int h) {
        int bg = down ? Gfx.alpha(c.t.accent(), 0.75 * m.alpha()) : Gfx.alpha(c.t.bg(), m.alpha());
        Gfx.rr(c.g, x, y, w, h, 4, bg);
        Gfx.ring(c.g, x, y, w, h, 4, Gfx.alpha(down ? c.t.accent() : c.t.border(), m.alpha()));
        Gfx.ctext(c.g, s.trim(), x + w / 2, y + (h - 8) / 2, down ? 0xFF101015 : c.t.text(), false);
    }

    public static int ping(Minecraft mc) {
        try {
            if (mc.getConnection() == null || mc.player == null) return 0;
            var pi = mc.getConnection().getPlayerInfo(mc.player.getUUID());
            return pi == null ? 0 : pi.getLatency();
        } catch (Exception e) {
            return 0;
        }
    }

    static String biome(Minecraft mc) {
        try {
            var key = mc.level.getBiome(mc.player.blockPosition()).unwrapKey();
            String n = key.map(k -> k.identifier().getPath()).orElse("?");
            return n.replace('_', ' ');
        } catch (Throwable e) {
            return "?";
        }
    }
}
