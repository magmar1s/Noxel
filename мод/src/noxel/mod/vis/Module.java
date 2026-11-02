package noxel.mod.vis;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;

/** Модуль визуалов. Рисует себя в точке (0,0); общий код сам переносит и масштабирует. */
public abstract class Module {
    public final String id, name, cat, desc;
    public final boolean defOn;
    public final double dx, dy;
    public final List<Opt> opts = new ArrayList<>();
    /** размер панели после последнего кадра */
    public int w, h;
    /** есть ли у модуля панель на экране (у «Крестика», «Виньетки» и т.п. своя отрисовка без перетаскивания) */
    public boolean panel = true;

    protected Module(String id, String name, String cat, String desc, boolean defOn, double dx, double dy) {
        this.id = id; this.name = name; this.cat = cat; this.desc = desc; this.defOn = defOn; this.dx = dx; this.dy = dy;
    }

    protected Module opt(Opt o) {
        opts.add(o);
        return this;
    }

    /** общие настройки панели */
    public Module common() {
        opts.add(0, Opt.num("alpha", "Прозрачность", 0.2, 1, 0.05, 1));
        opts.add(0, Opt.num("scale", "Размер", 0.6, 2, 0.1, 1));
        return this;
    }

    public boolean on() { return Cfg.enabled(id, defOn); }
    public double n(String k) {
        for (Opt o : opts) if (o.key.equals(k)) return Cfg.num(id, k, o.def);
        return 0;
    }
    public boolean b(String k) { return n(k) >= 0.5; }
    public int ch(String k) { return (int) Math.round(n(k)); }
    public double fx() { return Cfg.num(id, "x", dx); }
    public double fy() { return Cfg.num(id, "y", dy); }
    public double scale() { return panel ? Math.max(0.5, n("scale")) : 1; }
    public double alpha() { return panel ? Math.max(0.1, n("alpha")) : 1; }

    /** Данные, которые нужно собирать каждый кадр, даже если панель скрыта (счётчики кликов и т.п.) */
    public void frame(Minecraft mc) {}

    /** Рисует модуль в (0,0) и выставляет w, h. */
    public abstract void draw(Ctx c);

    // ── помощники для типовых панелей ──

    /** Список «подпись — значение» в стеклянной панели. */
    protected void list(Ctx c, String title, List<String[]> rows) {
        Theme t = c.t;
        boolean head = title != null && !title.isEmpty();
        int tw = head ? Gfx.tw(title) : 0, lw = 0, vw = 0;
        for (String[] r : rows) { lw = Math.max(lw, Gfx.tw(r[0])); vw = Math.max(vw, Gfx.tw(r[1])); }
        int inner = Math.max(tw + 14, lw + vw + (lw > 0 && vw > 0 ? 12 : 0));
        w = inner + 12;
        h = (head ? 14 : 0) + rows.size() * 11 + 8 - (head ? 0 : 0);
        if (rows.isEmpty()) h = (head ? 14 : 0) + 8;
        Gfx.glass(c.g, t, 0, 0, w, h, alpha());
        int y = 4;
        if (head) {
            Gfx.rr(c.g, 6, 6, 3, 6, 1, Gfx.alpha(t.accent(), alpha()));
            Gfx.text(c.g, title, 12, 4, Gfx.alpha(t.sub(), 0.5 + alpha() / 2), false);
            y = 17;
        }
        for (String[] r : rows) {
            int col = r.length > 2 ? Integer.parseUnsignedInt(r[2]) : t.text();
            Gfx.text(c.g, r[0], 6, y, Gfx.alpha(t.sub(), 0.5 + alpha() / 2), false);
            Gfx.text(c.g, r[1], w - 6 - Gfx.tw(r[1]), y, col, false);
            y += 11;
        }
    }

    protected static String[] row(String a, String b) { return new String[]{a, b}; }
    protected static String[] row(String a, String b, int color) { return new String[]{a, b, Integer.toUnsignedString(color)}; }

    /** Одна строка в маленькой «таблетке» с цветной точкой. */
    protected void pill(Ctx c, String text, int dot) {
        w = Gfx.tw(text) + (dot != 0 ? 22 : 14);
        h = 15;
        Gfx.glass(c.g, c.t, 0, 0, w, h, alpha());
        int x = 7;
        if (dot != 0) { Gfx.rr(c.g, 6, 5, 5, 5, 2, dot); x = 15; }
        Gfx.text(c.g, text, x, 3, c.t.text(), false);
    }
}
