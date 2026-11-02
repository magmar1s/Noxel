package noxel.mod.vis;

import java.util.ArrayList;
import java.util.List;
import java.util.function.DoubleConsumer;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.input.CharacterEvent;
import net.minecraft.client.input.KeyEvent;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.network.chat.Component;
import org.lwjgl.glfw.GLFW;

/** Внутриигровое меню визуалов: вкладки, поиск, карточки модулей с живым превью, стиль, конфиги, хранилище, метки. */
public final class MenuScreen extends Screen {
    private final Screen parent;
    private String tab = Cfg.has("ui", "seen") ? Cfg.gs("lastTab", Mods.INFO) : HOME;
    private boolean skipChar;
    private String tipName = "", tipDesc = "";
    private Module sel;
    private String search = "";
    private boolean searchFocus;
    private double scroll, scrollMax, scrollT;
    private long tabAt = System.currentTimeMillis();
    private final long opened = System.currentTimeMillis();
    private String msg = "";
    private long msgAt;

    private record Hit(int x, int y, int w, int h, DoubleConsumer click, DoubleConsumer drag) {}
    private final List<Hit> hits = new ArrayList<>();
    private Hit dragging;

    private static final String HOME = "Главная";
    private static final String[] TABS = {HOME, Mods.INFO, Mods.COMBAT, Mods.WORLD, Mods.SCREEN, Mods.MISC, Mods.ASSIST, "Хранилище", "Метки", "Стиль", "Конфиги"};

    public MenuScreen(Screen parent) {
        super(Component.literal("Noxel Visuals"));
        this.parent = parent;
    }

    @Override public boolean isPauseScreen() { return false; }
    @Override public void onClose() { Cfg.save(); minecraft.setScreenAndShow(parent); }
    @Override public boolean shouldCloseOnEsc() { return false; }

    void testDetail(String id) { sel = Mods.get(id); scroll = 0; }
    void testTab(String t) { tab = t; sel = null; scroll = 0; }

    /** Подсказка у курсора: полное название и описание модуля. */
    private void tooltip(GuiGraphicsExtractor g, Theme t, int mx, int my) {
        java.util.List<String> lines = new java.util.ArrayList<>();
        StringBuilder cur = new StringBuilder();
        for (String w : tipDesc.split(" ")) {
            if (Gfx.tw(cur + w) > 190 && cur.length() > 0) { lines.add(cur.toString().trim()); cur = new StringBuilder(); }
            cur.append(w).append(' ');
        }
        lines.add(cur.toString().trim());
        int bw = 0;
        for (String l : lines) bw = Math.max(bw, Gfx.tw(l));
        bw = Math.max(bw, Gfx.twb(tipName)) + 16;
        int bh = 18 + lines.size() * 10 + 6;
        int x = Math.min((int) (width / u()) - bw - 4, mx + 12), y = Math.min((int) (height / u()) - bh - 4, my + 14);
        Gfx.glass(g, t, x, y, bw, bh, 1);
        Gfx.btext(g, tipName, x + 8, y + 6, t.text());
        for (int i = 0; i < lines.size(); i++) Gfx.text(g, lines.get(i), x + 8, y + 18 + i * 10, t.sub(), false);
    }

    private void say(String s) { msg = s; msgAt = System.currentTimeMillis(); }

    // ── кирпичики интерфейса ──
    private void hit(int x, int y, int w, int h, DoubleConsumer click, DoubleConsumer drag) { hits.add(new Hit(x, y, w, h, click, drag)); }

    private boolean over(int x, int y, int w, int h, int mx, int my) { return mx >= x && mx < x + w && my >= y && my < y + h; }

    private int chip(GuiGraphicsExtractor g, Theme t, int x, int y, String s, boolean on, int mx, int my, Runnable r) {
        int w = Gfx.tw(s) + 12, h = 15;
        boolean hov = over(x, y, w, h, mx, my);
        Gfx.rr(g, x, y, w, h, 7, on ? Gfx.alpha(t.accent(), 0.85) : Gfx.alpha(0xFFFFFF, hov ? 0.16 : 0.08));
        Gfx.text(g, s, x + 6, y + 4, on ? 0xFF101015 : t.text(), false);
        hit(x, y, w, h, d -> r.run(), null);
        return w;
    }

    private int tgIdx;
    private void toggle(GuiGraphicsExtractor g, Theme t, int x, int y, boolean on, Runnable r) {
        float k = Anim.to("tg" + (tgIdx++ % 400) + ":" + x % 7 + ":" + (sel == null ? tab : sel.id), on ? 1 : 0, 16);
        Gfx.rr(g, x, y, 22, 11, 5, Gfx.mix(Gfx.alpha(0xFFFFFF, 0.18), Gfx.alpha(t.accent(), 0.9), k));
        int kx = x + 1 + Math.round(11 * k);
        Gfx.rr(g, kx, y + 1, 9, 9, 4, Gfx.mix(0xFFE8E8EE, 0xFF101015, k));
        hit(x - 2, y - 2, 26, 15, d -> r.run(), null);
    }

    private void slider(GuiGraphicsExtractor g, Theme t, int x, int y, int w, double v, double min, double max, DoubleConsumer set) {
        Gfx.rr(g, x, y + 3, w, 4, 2, Gfx.alpha(0xFFFFFF, 0.14));
        double k = (v - min) / (max - min);
        Gfx.rr(g, x, y + 3, Math.max(4, (int) (w * k)), 4, 2, t.accent());
        int kx = x + (int) (w * k);
        Gfx.rr(g, kx - 3, y, 7, 10, 3, 0xFFF4F4F8);
        DoubleConsumer f = mx -> set.accept(Math.max(min, Math.min(max, min + (mx - x) / (double) w * (max - min))));
        hit(x - 4, y - 2, w + 8, 14, f, f);
    }

    // ── кадр ──
    private float u() { return (float) Math.max(0.7, Math.min(1.6, Cfg.gn("ui", 1.0))); }

    @Override
    public void extractRenderState(GuiGraphicsExtractor g, int mx0, int my0, float delta) {
        float u = u();
        int mx = (int) (mx0 / u), my = (int) (my0 / u);
        int vw = (int) (width / u), vh = (int) (height / u);
        g.fill(0, 0, width, height, Gfx.argb((int) (120 * Math.min(1, (System.currentTimeMillis() - opened) / 160.0)), 0x000000));
        g.pose().pushMatrix();
        g.pose().scale(u, u);
        draw(g, mx, my, vw, vh);
        g.pose().popMatrix();
    }

    private void draw(GuiGraphicsExtractor g, int mx, int my, int width, int height) {
        Cfg.poll();
        tipName = ""; tipDesc = "";
        Anim.begin();
        tgIdx = 0;
        hits.clear();
        Theme t = Hud.theme();
        scrollT = Math.max(0, Math.min(scrollMax, scrollT));
        scroll += (scrollT - scroll) * Math.min(1, 16 * Anim.dt());
        if (Math.abs(scrollT - scroll) < 0.3) scroll = scrollT;
        double open = Math.min(1, (System.currentTimeMillis() - opened) / 160.0);
        int pw = Math.min(width - 12, 640), ph = Math.min(height - 12, 330);
        int px = (width - pw) / 2, py = (height - ph) / 2 + (int) ((1 - open) * 8);
        Gfx.glass(g, t, px, py, pw, ph, open);

        // боковая панель: логотип и разделы
        int sbw = 86;
        Gfx.rr(g, px + 8, py + 8, 4, 14, 2, t.accent());
        Gfx.btext(g, "Noxel", px + 16, py + 8, t.text());
        Gfx.text(g, "Visuals", px + 16, py + 17, t.sub(), false);
        int th = Math.max(12, Math.min(16, (ph - 60) / TABS.length));
        int ty = py + 32;
        for (int i = 0; i < TABS.length; i++) {
            String tb = TABS[i];
            if (i == 7) { g.fill(px + 10, ty + 1, px + 8 + sbw - 6, ty + 2, Gfx.alpha(t.border(), 0.7)); ty += 4; }   // дальше идут личные разделы
            boolean act = tab.equals(tb);
            boolean hov = over(px + 8, ty, sbw, th, mx, my);
            float tv = Anim.to("tab:" + tb, act ? 1 : hov ? 0.4f : 0, 14);
            if (act) { Gfx.rr(g, px + 8, ty, sbw, th, 7, Gfx.alpha(t.accent(), 0.22)); Gfx.ring(g, px + 8, ty, sbw, th, 7, Gfx.alpha(t.accent(), 0.8)); }
            else if (hov) Gfx.rr(g, px + 8, ty, sbw, th, 7, Gfx.alpha(0xFFFFFF, 0.08));
            int cnt = 0, onc = 0;
            for (Module m : Mods.ALL) if (m.cat.equals(tb)) { cnt++; if (m.on()) onc++; }
            Gfx.text(g, tb, px + 14, ty + (th - 8) / 2, act ? t.text() : t.sub(), false);
            if (cnt > 0) { String c = String.valueOf(onc); Gfx.text(g, c, px + 8 + sbw - 6 - Gfx.tw(c), ty + (th - 8) / 2, Gfx.alpha(t.sub(), 0.7), false); }
            String nm = tb;
            hit(px + 8, ty, sbw, th, d -> { tab = nm; sel = null; scroll = 0; scrollT = 0; tabAt = System.currentTimeMillis(); searchFocus = false; Cfg.setStyle("lastTab", nm); }, null);
            ty += th + 1;
        }
        g.fill(px + 8 + sbw + 3, py + 8, px + 8 + sbw + 4, py + ph - 20, Gfx.alpha(t.border(), 0.7));

        int bx = px + 8 + sbw + 10, by = py + 8, bw = pw - 16 - sbw - 10, bh = ph - 8 - 24;
        double ap = Anim.ease((System.currentTimeMillis() - tabAt) / 260.0);
        Gfx.fade = 0.15 + 0.85 * ap;
        g.enableScissor(bx, by, bx + bw, by + bh);
        g.pose().pushMatrix();
        g.pose().translate(0, (float) ((1 - ap) * 10));
        if (sel != null) detail(g, t, bx, by, bw, bh, mx, my);
        else switch (tab) {
            case HOME -> home(g, t, bx, by, bw, bh, mx, my);
            case "Хранилище" -> storage(g, t, bx, by, bw, bh, mx, my);
            case "Метки" -> waypoints(g, t, bx, by, bw, bh, mx, my);
            case "Стиль" -> style(g, t, bx, by, bw, bh, mx, my);
            case "Конфиги" -> configs(g, t, bx, by, bw, bh, mx, my);
            default -> {
                if (tab.equals(Mods.ASSIST)) { modules(g, t, bx, by, bw, bh - 46, mx, my); assistFooter(g, t, bx, by + bh - 42, bw, mx, my); }
                else modules(g, t, bx, by, bw, bh, mx, my);
            }
        }
        g.pose().popMatrix();
        g.disableScissor();
        Gfx.fade = 1;
        if (!tipName.isEmpty()) tooltip(g, t, mx, my);

        // низ
        long on = Mods.ALL.stream().filter(Module::on).count();
        Gfx.text(g, on + " из " + Mods.ALL.size() + " включено", px + 10, py + ph - 14, t.sub(), false);
        String hint = System.currentTimeMillis() - msgAt < 2500 ? msg : "Esc — закрыть   ·   правый Shift — меню";
        Gfx.text(g, hint, px + pw - 10 - Gfx.tw(hint), py + ph - 14, System.currentTimeMillis() - msgAt < 2500 ? t.accent() : t.sub(), false);
    }

    // ── главная: быстрый старт, итоги, горячие клавиши ──
    private void home(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        long on = Mods.ALL.stream().filter(Module::on).count();
        boolean first = !Cfg.has("ui", "seen");
        Gfx.btext(g, first ? "Добро пожаловать в Noxel Visuals" : "Noxel Visuals", x, y + 2, t.text());
        Gfx.text(g, first ? "С чего начать? Выбери набор: потом всё можно поменять и вернуть назад." : on + " из " + Mods.ALL.size() + " модулей включено", x, y + 15, t.sub(), false);
        int cy = y + 32, cw = (w - 6) / 2, ch = 46, i = 0;
        for (var e : Presets.ALL.keySet()) {
            int cx = x + (i % 2) * (cw + 6), yy = cy + (i / 2) * (ch + 5);
            boolean hov = over(cx, yy, cw, ch, mx, my);
            float hv = Anim.to("pr:" + e, hov ? 1 : 0, 14);
            Gfx.rr(g, cx, yy, cw, ch, 9, Gfx.alpha(0xFFFFFF, 0.05 + 0.1 * hv));
            Gfx.ring(g, cx, yy, cw, ch, 9, Gfx.mix(Gfx.alpha(t.border(), 0.6), Gfx.alpha(t.accent(), 0.9), hv));
            Gfx.text(g, e, cx + 10, yy + 7, t.text(), false);
            String d = Presets.DESC.get(e); int ly = yy + 19;
            for (int ln = 0; ln < 2 && !d.isEmpty(); ln++) {
                int cut = d.length();
                while (Gfx.tw(d.substring(0, cut)) > cw - 20 && cut > 1) { cut = d.lastIndexOf(' ', cut - 1); if (cut <= 0) { cut = Math.max(1, Math.min(d.length(), 12)); break; } }
                Gfx.text(g, d.substring(0, cut).trim(), cx + 10, ly, t.sub(), false);
                d = d.substring(cut).trim(); ly += 10;
            }
            String key = e;
            hit(cx, yy, cw, ch, d2 -> { Cfg.snapshotUndo(); Presets.apply(key); Cfg.setStyle("seen", 1); Cfg.markSeen(); say("Набор «" + key + "» включён · можно вернуть назад"); Stats.notify("Набор «" + key + "»", 0xFF5BE3A8); }, null);
            i++;
        }
        cy += ((i + 1) / 2) * (ch + 5) + 4;
        int cx = x;
        cx += chip(g, t, cx, cy, "Расставить панели", false, mx, my, () -> minecraft.setScreenAndShow(new EditScreen(this))) + 4;
        cx += chip(g, t, cx, cy, "Вернуть как было", false, mx, my, () -> say(Cfg.undo() ? "Вернул прошлые настройки" : "Возвращать пока нечего")) + 4;
        cx += chip(g, t, cx, cy, "Скопировать код", false, mx, my, () -> { minecraft.keyboardHandler.setClipboard(Cfg.export()); say("Код скопирован: отправь другу"); }) + 4;
        cy += 22;
        Gfx.text(g, "Клавиши:  Правый Shift — меню   ·   B — метка   ·   C — приближение   ·   G — осмотр   ·   V — координаты в чат", x, cy, Gfx.alpha(t.sub(), 0.9), false);
    }

    // ── вкладка со списком модулей: карточки с живым превью ──
    private void modules(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        int sw2 = Math.min(190, w / 2);
        Gfx.rr(g, x, y, sw2, 16, 8, Gfx.alpha(0xFFFFFF, searchFocus ? 0.16 : 0.08));
        if (searchFocus) Gfx.ring(g, x, y, sw2, 16, 8, Gfx.alpha(t.accent(), 0.8));
        Gfx.text(g, search.isEmpty() && !searchFocus ? "Поиск по всем модулям (нажми /)" : search + (searchFocus && System.currentTimeMillis() / 400 % 2 == 0 ? "|" : ""), x + 8, y + 4, search.isEmpty() ? t.sub() : t.text(), false);
        hit(x, y, sw2, 16, d -> searchFocus = true, null);
        List<Module> list = new ArrayList<>();
        for (Module m : Mods.ALL) {
            boolean ok = search.isBlank() ? m.cat.equals(tab) : (m.name + " " + m.desc).toLowerCase().contains(search.toLowerCase());
            if (ok) list.add(m);
        }
        long en = list.stream().filter(Module::on).count();
        String c = en + " из " + list.size() + " включено";
        Gfx.text(g, c, x + w - Gfx.tw(c), y + 4, t.sub(), false);
        int cols = w >= 430 ? 2 : 1;
        int cw = (w - 6 * (cols - 1)) / cols, ch = 56, top = y + 22;
        int rows = (list.size() + cols - 1) / cols;
        scrollMax = Math.max(0, rows * (ch + 5) - (h - 22));
        scrollT = Math.max(0, Math.min(scrollMax, scrollT));
        scroll = Math.max(0, Math.min(scrollMax, scroll));
        g.enableScissor(x, top, x + w, y + h);
        Ctx pc = new Ctx(g, t, minecraft, true);
        for (int i = 0; i < list.size(); i++) {
            Module m = list.get(i);
            int cx = x + (i % cols) * (cw + 6), cy = top + (i / cols) * (ch + 5) - (int) scroll;
            if (cy + ch < top || cy > y + h) continue;
            boolean hov = over(cx, cy, cw, ch, mx, my) && my >= top;
            boolean on = m.on();
            float hv = Anim.to("c:" + m.id, hov ? 1 : 0, 14);
            Gfx.rr(g, cx, cy, cw, ch, 10, Gfx.alpha(0xFFFFFF, 0.05 + 0.09 * hv));
            Gfx.ring(g, cx, cy, cw, ch, 10, Gfx.mix(Gfx.alpha(t.border(), 0.55), Gfx.alpha(t.accent(), 0.85), on ? 1f : hv * 0.6f));
            int tw = cw - 98;
            Gfx.text(g, m.name, cx + 10, cy + 7, t.text(), false);
            // описание в две строки
            String d = m.desc; int ly = cy + 20;
            for (int ln = 0; ln < 2 && !d.isEmpty(); ln++) {
                int cut = d.length();
                while (Gfx.tw(d.substring(0, cut)) > tw && cut > 1) { cut = d.lastIndexOf(' ', cut - 1); if (cut <= 0) { cut = Math.max(1, Math.min(d.length(), 10)); break; } }
                String line = d.substring(0, cut).trim();
                d = d.substring(cut).trim();
                if (ln == 1 && !d.isEmpty()) line = line.length() > 2 ? line.substring(0, line.length() - 1) + "…" : line;
                Gfx.text(g, line, cx + 10, ly, t.sub(), false);
                ly += 10;
            }
            toggle(g, t, cx + 10, cy + ch - 15, on, () -> { Cfg.setEnabled(m.id, !m.on()); Stats.notify(m.name + (m.on() ? ": включено" : ": выключено"), m.on() ? 0xFF5BE3A8 : 0xFFFF6B6B); });
            Gfx.text(g, on ? "включено" : "выключено", cx + 38, cy + ch - 14, on ? t.accent() : Gfx.alpha(t.sub(), 0.8), false);
            // живое превью справа
            int px = cx + cw - 86, py = cy + 6, pw = 78, ph = ch - 12;
            Gfx.rr(g, px, py, pw, ph, 7, Gfx.alpha(0x000000, 0.32));
            g.enableScissor(Math.max(px, x), Math.max(py, top), Math.min(px + pw, x + w), Math.min(py + ph, y + h));
            if (m.panel) {
                if (m.w <= 0 || m.h <= 0) { g.pose().pushMatrix(); try { m.draw(pc); } catch (Throwable ex) { } g.pose().popMatrix(); }
                float s = (float) Math.min(1.0, Math.min((pw - 8) / (double) Math.max(1, m.w), (ph - 8) / (double) Math.max(1, m.h)));
                g.pose().pushMatrix();
                g.pose().translate(px + (pw - m.w * s) / 2f, py + (ph - m.h * s) / 2f);
                g.pose().scale(s, s);
                try { m.draw(pc); } catch (Throwable ex) { }
                g.pose().popMatrix();
            } else if (m.id.equals("crosshair")) {
                Hud.drawCrosshair(pc, m, px + pw / 2, py + ph / 2);
            } else {
                Gfx.ctext(g, m.opts.isEmpty() ? "в игре" : "⚙ настройки", px + pw / 2, py + ph / 2 - 4, Gfx.alpha(t.sub(), 0.8), false);
            }
            g.disableScissor();
            g.enableScissor(x, top, x + w, y + h);
            hit(cx + 80, cy, cw - 80, ch, d2 -> { sel = m; scroll = 0; scrollT = 0; tabAt = System.currentTimeMillis(); }, null);
            hit(cx, cy, 80, ch - 20, d2 -> { sel = m; scroll = 0; scrollT = 0; tabAt = System.currentTimeMillis(); }, null);
            if (hov && my >= top) { tipName = m.name; tipDesc = m.desc; }
        }
        g.disableScissor();
    }

    private void assistFooter(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int mx, int my) {
        g.fill(x, y - 2, x + w, y - 1, Gfx.alpha(t.border(), 0.6));
        Gfx.text(g, "⚠ Помощники — это автоматизация: на многих серверах они запрещены. Читай правила сервера. Список ниже отключает их там насовсем.", x, y + 2, 0xFFFFC14D, false);
        var sd = minecraft.getCurrentServer();
        int cx = x;
        if (sd != null) cx += chip(g, t, cx, y + 15, "Запретить помощников на " + sd.ip, false, mx, my, () -> { var a = Cfg.arr("assistBlock"); a.add(sd.ip); Cfg.touch(); say("Добавлено в запрещённые"); }) + 6;
        var list = Cfg.arr("assistBlock");
        for (int i = 0; i < list.size() && i < 4; i++) {
            String ip = list.get(i).getAsString();
            int idx = i;
            cx += chip(g, t, cx, y + 15, ip + "  ×", true, mx, my, () -> { list.remove(idx); Cfg.touch(); }) + 4;
            if (cx > x + w - 80) break;
        }
        if (sd == null && list.size() == 0) Gfx.text(g, "Зайди на сервер, и здесь появится кнопка «Запретить помощников на этом сервере».", x, y + 19, t.sub(), false);
    }

    // ── страница модуля ──
    private void detail(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        Module m = sel;
        chip(g, t, x, y, "‹ Назад", false, mx, my, () -> { sel = null; scroll = 0; });
        Gfx.text(g, m.name, x + 60, y + 4, t.text(), true);
        toggle(g, t, x + w - 24, y + 2, m.on(), () -> Cfg.setEnabled(m.id, !m.on()));
        // превью
        int pw = Math.min(170, w / 2 - 6), ph = 96, py = y + 22;
        Gfx.rr(g, x, py, pw, ph, 8, Gfx.alpha(0x000000, 0.35));
        g.fillGradient(x + 1, py + 1, x + pw - 1, py + ph - 1, 0x401A2A44, 0x40253018);
        Gfx.ring(g, x, py, pw, ph, 8, Gfx.alpha(t.border(), 0.7));
        Ctx pc = new Ctx(g, t, minecraft, true);
        g.enableScissor(x + 2, py + 2, x + pw - 2, py + ph - 2);
        if (m.panel) {
            g.pose().pushMatrix();
            if (m.w <= 0) { try { m.draw(pc); } catch (Throwable ex) { } }
            float ps = (float) Math.min(1.0, Math.min((pw - 16) / (double) Math.max(1, m.w), (ph - 16) / (double) Math.max(1, m.h)));
            g.pose().translate(x + (pw - m.w * ps) / 2f, py + (ph - m.h * ps) / 2f);
            g.pose().scale(ps, ps);
            try { m.draw(pc); } catch (Throwable ex) { }
            g.pose().popMatrix();
        } else if (m.id.equals("crosshair")) {
            Hud.drawCrosshair(pc, m, x + pw / 2, py + ph / 2);
        } else {
            Gfx.ctext(g, "Работает прямо в игре", x + pw / 2, py + ph / 2 - 4, t.sub(), false);
        }
        g.disableScissor();
        int ty = py + ph + 6;
        String[] words = m.desc.split(" ");
        StringBuilder line = new StringBuilder();
        for (String wd : words) {
            if (Gfx.tw(line + wd) > pw) { Gfx.text(g, line.toString(), x, ty, t.sub(), false); ty += 10; line = new StringBuilder(); }
            line.append(wd).append(' ');
        }
        Gfx.text(g, line.toString(), x, ty, t.sub(), false);
        ty += 16;
        if (m.panel) {
            chip(g, t, x, ty, "Переместить", false, mx, my, () -> minecraft.setScreenAndShow(new EditScreen(this)));
            chip(g, t, x + Gfx.tw("Переместить") + 16, ty, "Сбросить", false, mx, my, () -> { Cfg.reset(m.id); say("Настройки модуля сброшены"); });
        } else {
            chip(g, t, x, ty, "Сбросить", false, mx, my, () -> { Cfg.reset(m.id); say("Настройки модуля сброшены"); });
        }
        // настройки
        int ox = x + pw + 12, ow = w - pw - 12, oy = y + 22 - (int) scroll;
        int total = m.opts.size() * 22;
        scrollMax = Math.max(0, total - (h - 22));
        scroll = Math.max(0, Math.min(scrollMax, scroll));
        g.enableScissor(ox, y + 20, x + w, y + h);
        if (m.opts.isEmpty()) Gfx.text(g, "У этого модуля нет настроек: включил и работает.", ox, oy + 4, t.sub(), false);
        for (Opt o : m.opts) {
            Gfx.rr(g, ox, oy, ow, 19, 6, Gfx.alpha(0xFFFFFF, 0.05));
            Gfx.text(g, o.name, ox + 7, oy + 5, t.text(), false);
            switch (o.kind) {
                case BOOL -> toggle(g, t, ox + ow - 29, oy + 4, m.b(o.key), () -> Cfg.setNum(m.id, o.key, m.b(o.key) ? 0 : 1));
                case NUM -> {
                    double v = m.n(o.key);
                    String vs = o.step >= 1 ? String.valueOf((int) Math.round(v)) : String.format("%.2f", v);
                    Gfx.text(g, vs, ox + ow - 8 - Gfx.tw(vs), oy + 5, t.sub(), false);
                    int cw2 = Math.max(40, Math.min(120, ow - Gfx.tw(o.name) - 70));
                    int sx = ox + ow - 46 - cw2;
                    slider(g, t, sx, oy + 4, cw2, v, o.min, o.max, nv -> Cfg.setNum(m.id, o.key, Math.round(nv / o.step) * o.step));
                }
                case CHOICE -> {
                    String cur = o.choices[Math.max(0, Math.min(o.choices.length - 1, m.ch(o.key)))];
                    int cw2 = Gfx.tw(cur) + 14;
                    int cx = ox + ow - 6 - cw2;
                    Gfx.rr(g, cx, oy + 2, cw2, 15, 7, Gfx.alpha(t.accent(), 0.3));
                    Gfx.text(g, cur, cx + 7, oy + 6, t.text(), false);
                    hit(cx, oy + 2, cw2, 15, d -> Cfg.setNum(m.id, o.key, (m.ch(o.key) + 1) % o.choices.length), null);
                }
            }
            oy += 22;
        }
        g.disableScissor();
    }

    // ── хранилище ──
    private void storage(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        Gfx.rr(g, x, y, Math.min(220, w - 60), 15, 7, Gfx.alpha(0xFFFFFF, searchFocus ? 0.16 : 0.08));
        if (searchFocus) Gfx.ring(g, x, y, Math.min(220, w - 60), 15, 7, Gfx.alpha(t.accent(), 0.8));
        String q = Storage.search;
        Gfx.text(g, q.isEmpty() && !searchFocus ? "Найти предмет: diamond, totem, netherite…" : q + (searchFocus && System.currentTimeMillis() / 400 % 2 == 0 ? "|" : ""), x + 7, y + 4, q.isEmpty() ? t.sub() : t.text(), false);
        hit(x, y, Math.min(220, w - 60), 15, d -> searchFocus = true, null);
        var all = Storage.all(minecraft);
        Gfx.text(g, all.size() + " хранилищ в памяти", x + w - Gfx.tw(all.size() + " хранилищ в памяти"), y + 4, t.sub(), false);
        int top = y + 22;
        List<Storage.Rec> list = new ArrayList<>();
        for (var r : all) if (Storage.matches(r, q)) list.add(r);
        list.sort((a, b) -> Long.compare(b.at(), a.at()));
        if (all.isEmpty()) Gfx.text(g, "Открой любой сундук: Noxel запомнит его содержимое.", x, top + 4, t.sub(), false);
        scrollMax = Math.max(0, list.size() * 25 - (h - 22));
        scroll = Math.max(0, Math.min(scrollMax, scroll));
        g.enableScissor(x, top, x + w, y + h);
        int cy = top - (int) scroll;
        for (var r : list) {
            Gfx.rr(g, x, cy, w, 22, 6, Gfx.alpha(0xFFFFFF, 0.06));
            double dist = minecraft.player == null ? 0 : Math.sqrt(minecraft.player.distanceToSqr(r.x() + 0.5, r.y() + 0.5, r.z() + 0.5));
            Gfx.text(g, r.x() + " " + r.y() + " " + r.z() + "   " + (int) dist + " м", x + 7, cy + 3, t.text(), false);
            StringBuilder sb = new StringBuilder();
            int k = 0;
            for (var e : r.items().entrySet()) { if (k++ >= 4) { sb.append("…"); break; } sb.append(e.getKey().replace('_', ' ')).append(" ×").append(e.getValue()).append("   "); }
            if (r.items().isEmpty()) sb.append("пусто");
            String s = sb.toString();
            while (Gfx.tw(s) > w - 70 && s.length() > 3) s = s.substring(0, s.length() - 2);
            Gfx.text(g, s, x + 7, cy + 12, t.sub(), false);
            Gfx.text(g, "забыть", x + w - 7 - Gfx.tw("забыть"), cy + 7, 0xFFFF8B8B, false);
            hit(x + w - 50, cy, 50, 22, d -> Storage.forget(r), null);
            cy += 25;
        }
        g.disableScissor();
    }

    // ── метки ──
    private void waypoints(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        chip(g, t, x, y, "Поставить метку здесь (B)", false, mx, my, () -> Waypoints.add(minecraft));
        chip(g, t, x + Gfx.tw("Поставить метку здесь (B)") + 18, y, "Удалить все метки этого мира", false, mx, my, () -> { Waypoints.clear(minecraft); say("Метки удалены"); });
        int cy = y + 22;
        int n = 0;
        var arr = Cfg.arr("waypoints");
        for (int i = 0; i < arr.size(); i++) {
            var o = arr.get(i).getAsJsonObject();
            if (!o.get("k").getAsString().equals(Waypoints.key(minecraft))) continue;
            Gfx.rr(g, x, cy, w, 19, 6, Gfx.alpha(0xFFFFFF, 0.06));
            Gfx.text(g, o.get("n").getAsString() + "    " + o.get("x").getAsInt() + " " + o.get("y").getAsInt() + " " + o.get("z").getAsInt(), x + 7, cy + 5, t.text(), false);
            int idx = i;
            Gfx.text(g, "удалить", x + w - 7 - Gfx.tw("удалить"), cy + 5, 0xFFFF8B8B, false);
            hit(x + w - 50, cy, 50, 19, d -> { arr.remove(idx); Cfg.touch(); }, null);
            cy += 22;
            if (++n > 9) break;
        }
        if (n == 0) Gfx.text(g, "Меток пока нет. Нажми B в игре, чтобы поставить первую.", x, cy + 4, t.sub(), false);
    }

    // ── стиль ──
    private void style(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        int cy = y;
        Gfx.text(g, "Стиль стекла", x, cy + 3, t.sub(), false);
        int cx = x + 90;
        for (String s : Theme.STYLES) cx += chip(g, t, cx, cy, s, Cfg.gs("style", "Жидкое стекло").equals(s), mx, my, () -> Cfg.setStyle("style", s)) + 3;
        cy += 22;
        Gfx.text(g, "Цвет акцента", x, cy + 3, t.sub(), false);
        cx = x + 90;
        for (String s : Theme.ACCENTS) {
            Theme tt = Theme.of("Тёмное стекло", s, 5, 1);
            boolean on = Cfg.gs("accent", "Фиолетовый").equals(s);
            Gfx.rr(g, cx, cy, 15, 15, 7, tt.accent());
            if (on) Gfx.ring(g, cx - 2, cy - 2, 19, 19, 9, 0xFFFFFFFF);
            hit(cx, cy, 15, 15, d -> Cfg.setStyle("accent", s), null);
            cx += 22;
        }
        cy += 22;
        Gfx.text(g, "Скругление", x, cy + 1, t.sub(), false);
        slider(g, t, x + 90, cy, 120, Cfg.gn("radius", 7), 0, 9, v -> Cfg.setStyle("radius", Math.round(v)));
        cy += 20;
        Gfx.text(g, "Плотность стекла", x, cy + 1, t.sub(), false);
        slider(g, t, x + 90, cy, 120, Cfg.gn("glass", 0.9), 0.2, 1, v -> Cfg.setStyle("glass", Math.round(v * 20) / 20.0));
        cy += 20;
        Gfx.text(g, "Размер меню", x, cy + 1, t.sub(), false);
        slider(g, t, x + 90, cy, 120, Cfg.gn("ui", 1.0), 0.7, 1.6, v -> Cfg.setStyle("ui", Math.round(v * 20) / 20.0));
        cy += 24;
        Gfx.text(g, "Так выглядят панели:", x, cy, t.sub(), false);
        Ctx pc = new Ctx(g, t, minecraft, true);
        g.pose().pushMatrix();
        g.pose().translate(x, cy + 12);
        Mods.get("island").draw(pc);
        g.pose().translate(0, 22);
        Mods.get("keys").draw(pc);
        g.pose().popMatrix();
        g.pose().pushMatrix();
        g.pose().translate(x + 150, cy + 12);
        Mods.get("target").draw(pc);
        g.pose().translate(0, 40);
        Mods.get("potions").draw(pc);
        g.pose().popMatrix();
    }

    // ── конфиги ──
    private void configs(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, int mx, int my) {
        int cy = y;
        Gfx.text(g, "Готовые наборы: один клик включает нужные модули", x, cy, t.sub(), false);
        cy += 12;
        int cx = x;
        for (String p : Presets.ALL.keySet()) {
            int cw = chip(g, t, cx, cy, p, false, mx, my, () -> { Presets.apply(p); say("Набор «" + p + "» включён"); Stats.notify("Набор «" + p + "»", 0xFF5BE3A8); });
            cx += cw + 4;
            if (cx > x + w - 60) { cx = x; cy += 18; }
        }
        cy += 20;
        if (over(x, y, w, h, mx, my)) {
            // подсказка к набору под курсором
            int hx = x, hy = y + 12;
            for (String p : Presets.ALL.keySet()) {
                int cw = Gfx.tw(p) + 12;
                if (over(hx, hy, cw, 15, mx, my)) { Gfx.text(g, Presets.DESC.get(p), x, cy, t.accent(), false); }
                hx += cw + 4;
            }
        }
        cy += 14;
        g.fill(x, cy, x + w, cy + 1, Gfx.alpha(t.border(), 0.6));
        cy += 8;
        Gfx.text(g, "Поделиться настройками: код копируется в буфер обмена", x, cy, t.sub(), false);
        cy += 12;
        cx = x;
        cx += chip(g, t, cx, cy, "Скопировать мой код", false, mx, my, () -> { minecraft.keyboardHandler.setClipboard(Cfg.export()); say("Код скопирован: отправь его другу"); }) + 4;
        cx += chip(g, t, cx, cy, "Вставить код из буфера", false, mx, my, () -> say(Cfg.importCode(minecraft.keyboardHandler.getClipboard()) ? "Настройки применены" : "В буфере нет кода NXV1")) + 4;
        chip(g, t, cx, cy, "Сбросить всё", false, mx, my, () -> { Cfg.replaceAll(new com.google.gson.JsonObject()); say("Всё сброшено"); });
        cy += 24;
        Gfx.text(g, "Файл: " + Cfg.path().getFileName() + "  ·  его же правит страница «Визуалы» в лаунчере", x, cy, Gfx.alpha(t.sub(), 0.8), false);
    }

    // ── ввод ──
    @Override
    public boolean mouseClicked(MouseButtonEvent e, boolean dbl) {
        searchFocus = false;
        for (int i = hits.size() - 1; i >= 0; i--) {
            Hit h = hits.get(i);
            if (over(h.x, h.y, h.w, h.h, (int) (e.x() / u()), (int) (e.y() / u()))) {
                h.click.accept(e.x() / u());
                if (h.drag == null) Snd.ui(0.5f);
                dragging = h.drag != null ? h : null;
                return true;
            }
        }
        return super.mouseClicked(e, dbl);
    }

    @Override
    public boolean mouseDragged(MouseButtonEvent e, double dx, double dy) {
        if (dragging != null) { dragging.drag.accept(e.x() / u()); return true; }
        return super.mouseDragged(e, dx, dy);
    }

    @Override
    public boolean mouseReleased(MouseButtonEvent e) {
        dragging = null;
        return super.mouseReleased(e);
    }

    @Override
    public boolean mouseScrolled(double mx, double my, double sx, double sy) {
        scrollT = Math.max(0, Math.min(scrollMax, scrollT - sy * 22));
        return true;
    }

    @Override
    public boolean charTyped(CharacterEvent e) {
        if (!searchFocus) return super.charTyped(e);
        String s = e.codepointAsString();
        if (skipChar) { skipChar = false; if (s.equals("/")) return true; }
        if (tab.equals("Хранилище") && sel == null) Storage.search += s; else search += s;
        scroll = 0;
        return true;
    }

    @Override
    public boolean keyPressed(KeyEvent e) {
        if (!searchFocus && e.key() == GLFW.GLFW_KEY_SLASH && sel == null) { searchFocus = true; skipChar = true; if (!tab.equals("Хранилище") && !java.util.Arrays.asList(TABS).subList(1, 7).contains(tab)) tab = Mods.INFO; return true; }
        if (!searchFocus && (e.key() == GLFW.GLFW_KEY_DOWN || e.key() == GLFW.GLFW_KEY_UP) && sel == null) {
            int i = java.util.Arrays.asList(TABS).indexOf(tab);
            i = (i + (e.key() == GLFW.GLFW_KEY_DOWN ? 1 : TABS.length - 1)) % TABS.length;
            tab = TABS[i]; scroll = 0; scrollT = 0; tabAt = System.currentTimeMillis(); Cfg.setStyle("lastTab", tab); Snd.ui(0.4f);
            return true;
        }
        if (e.key() == GLFW.GLFW_KEY_ESCAPE) {
            if (searchFocus) { searchFocus = false; return true; }
            if (sel != null) { sel = null; scroll = 0; return true; }
            onClose();
            return true;
        }
        if (Hud.menuKey != null && Hud.menuKey.matches(e) && !searchFocus) { onClose(); return true; }
        if (searchFocus && e.key() == GLFW.GLFW_KEY_BACKSPACE) {
            if (tab.equals("Хранилище") && sel == null) { if (!Storage.search.isEmpty()) Storage.search = Storage.search.substring(0, Storage.search.length() - 1); }
            else if (!search.isEmpty()) search = search.substring(0, search.length() - 1);
            return true;
        }
        return super.keyPressed(e);
    }
}
