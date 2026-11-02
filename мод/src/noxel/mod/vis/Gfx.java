package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;

/** Рисование в стиле «стекло»: скруглённые панели, рамки, полосы, цвета. Только заливки: работает без миксинов. */
public final class Gfx {
    private Gfx() {}

    /** общая прозрачность: меню плавно проявляет содержимое при смене вкладки */
    public static double fade = 1;
    /** у жидкого стекла текст светлый, поэтому всегда с тенью */
    public static boolean forceShadow;

    public static int argb(int a, int rgb) {
        return (Math.max(0, Math.min(255, a)) << 24) | (rgb & 0xFFFFFF);
    }

    public static int alpha(int c, double k) {
        int a = (int) Math.round(((c >>> 24) & 255) * k);
        return (Math.max(0, Math.min(255, a)) << 24) | (c & 0xFFFFFF);
    }

    public static int mix(int a, int b, double t) {
        t = Math.max(0, Math.min(1, t));
        int r = 0;
        for (int s = 0; s <= 24; s += 8) r |= (int) Math.round(((a >>> s) & 255) * (1 - t) + ((b >>> s) & 255) * t) << s;
        return r;
    }

    public static int hsv(double h, double s, double v) {
        h = ((h % 1) + 1) % 1;
        double c = v * s, x = c * (1 - Math.abs((h * 6) % 2 - 1)), m = v - c;
        double r, g, b;
        int k = (int) (h * 6);
        switch (k) {
            case 0 -> { r = c; g = x; b = 0; }
            case 1 -> { r = x; g = c; b = 0; }
            case 2 -> { r = 0; g = c; b = x; }
            case 3 -> { r = 0; g = x; b = c; }
            case 4 -> { r = x; g = 0; b = c; }
            default -> { r = c; g = 0; b = x; }
        }
        return 0xFF000000 | (int) Math.round((r + m) * 255) << 16 | (int) Math.round((g + m) * 255) << 8 | (int) Math.round((b + m) * 255);
    }

    private static int inset(int r, int i) {
        double d = r - i - 0.5;
        return r - (int) Math.round(Math.sqrt(Math.max(0, r * r - d * d)));
    }

    /** Скруглённый прямоугольник (полупрозрачность не накладывается: строки не пересекаются). */
    public static void rr(GuiGraphicsExtractor g, int x, int y, int w, int h, int r, int c) {
        if (fade < 1) c = alpha(c, fade);
        if (w <= 0 || h <= 0 || (c >>> 24) == 0) return;
        r = Math.max(0, Math.min(r, Math.min(w, h) / 2));
        for (int i = 0; i < r; i++) {
            int o = inset(r, i);
            g.fill(x + o, y + i, x + w - o, y + i + 1, c);
            g.fill(x + o, y + h - 1 - i, x + w - o, y + h - i, c);
        }
        g.fill(x, y + r, x + w, y + h - r, c);
    }

    /** Тонкая рамка по контуру скруглённого прямоугольника. */
    public static void ring(GuiGraphicsExtractor g, int x, int y, int w, int h, int r, int c) {
        if (fade < 1) c = alpha(c, fade);
        if (w <= 2 || h <= 2 || (c >>> 24) == 0) return;
        r = Math.max(0, Math.min(r, Math.min(w, h) / 2));
        int prev = r > 0 ? inset(r, 0) : 0;
        g.fill(x + prev, y, x + w - prev, y + 1, c);
        g.fill(x + prev, y + h - 1, x + w - prev, y + h, c);
        for (int i = 1; i < r; i++) {
            int o = inset(r, i), to = Math.max(o + 1, prev);
            g.fill(x + o, y + i, x + to, y + i + 1, c);
            g.fill(x + w - to, y + i, x + w - o, y + i + 1, c);
            g.fill(x + o, y + h - 1 - i, x + to, y + h - i, c);
            g.fill(x + w - to, y + h - 1 - i, x + w - o, y + h - i, c);
            prev = o;
        }
        g.fill(x, y + r, x + 1, y + h - r, c);
        g.fill(x + w - 1, y + r, x + w, y + h - r, c);
    }

    /** Построчная заливка скруглённой панели вертикальным градиентом с бликом сверху. */
    private static void rrGrad(GuiGraphicsExtractor g, int x, int y, int w, int h, int r, int top, int bot, double spec) {
        if (w <= 0 || h <= 0) return;
        r = Math.max(0, Math.min(r, Math.min(w, h) / 2));
        for (int i = 0; i < h; i++) {
            int o = i < r ? inset(r, i) : i >= h - r ? inset(r, h - 1 - i) : 0;
            double k = i / (double) Math.max(1, h - 1);
            int c = mix(top, bot, k);
            if (spec > 0 && k < 0.45) c = mix(c, 0xFFFFFFFF, spec * (1 - k / 0.45) * 0.55);
            if (fade < 1) c = alpha(c, fade);
            g.fill(x + o, y + i, x + w - o, y + i + 1, c);
        }
    }

    /** Рамка с градиентом: сверху яркая, снизу тусклая (свет падает сверху-слева, как у настоящего стекла). */
    private static void ringGrad(GuiGraphicsExtractor g, int x, int y, int w, int h, int r, int c1, int c2) {
        if (w <= 2 || h <= 2) return;
        r = Math.max(0, Math.min(r, Math.min(w, h) / 2));
        int prev = r > 0 ? inset(r, 0) : 0;
        int ct = fade < 1 ? alpha(c1, fade) : c1, cb = fade < 1 ? alpha(c2, fade) : c2;
        g.fill(x + prev, y, x + w - prev, y + 1, ct);
        g.fill(x + prev, y + h - 1, x + w - prev, y + h, cb);
        for (int i = 1; i < h - 1; i++) {
            int o, to;
            if (i < r) { o = inset(r, i); to = Math.max(o + 1, prev); prev = o; }
            else if (i >= h - r) { o = inset(r, h - 1 - i); to = Math.max(o + 1, inset(r, h - 2 - i)); }
            else { o = 0; to = 1; }
            double k = i / (double) (h - 1);
            int c = mix(c1, c2, Math.min(1, k * 1.15));
            if (fade < 1) c = alpha(c, fade);
            g.fill(x + o, y + i, x + to, y + i + 1, c);
            g.fill(x + w - to, y + i, x + w - o, y + i + 1, c);
        }
    }

    /** Стеклянная панель: фон, блик сверху, рамка, лёгкая тень. */
    public static void glass(GuiGraphicsExtractor g, Theme t, int x, int y, int w, int h, double k) {
        int r = t.radius();
        if (t.liquid()) {
            // мягкая тень под стеклом
            rr(g, x + 1, y + 3, w - 2, h, r, argb((int) (30 * k), 0));
            rr(g, x - 1, y + 2, w + 2, h + 1, r + 1, argb((int) (18 * k), 0));
            rr(g, x - 2, y + 1, w + 4, h + 3, r + 2, argb((int) (9 * k), 0));
            rrGrad(g, x, y, w, h, r, alpha(t.top(), k), alpha(t.bot(), k), t.spec());
            // двойная кромка: внутренний тонкий отблеск и яркий внешний ободок
            ringGrad(g, x + 1, y + 1, w - 2, h - 2, Math.max(0, r - 1), alpha(argb(95, 0xFFFFFF), k), alpha(argb(26, 0xFFFFFF), k));
            ringGrad(g, x, y, w, h, r, alpha(t.edge1(), k), alpha(t.edge2(), k));
            int gl = Math.max(5, w / 6);
            g.fill(x + r + 1, y + 1, x + r + 1 + gl, y + 2, alpha(0xFFFFFFFF, k));                                   // блик сверху слева
            g.fill(x + w - r - 1 - gl, y + h - 2, x + w - r - 1, y + h - 1, alpha(argb(235, 0xFFFFFF), k));          // блик снизу справа
            if (h > 18) g.fill(x + 1, y + r + 2, x + 2, y + r + 2 + Math.max(4, h / 4), alpha(argb(170, 0xFFFFFF), k));
            g.fill(x + r + 2, y + h - 1, x + w - r - 2, y + h, alpha(mix(argb(200, 0xFFFFFF), t.accent(), 0.45), k * 0.9));  // цветная кайма по низу
            return;
        }
        if (t.shadow()) rr(g, x + 1, y + 2, w, h, r, argb((int) (60 * k), 0));
        rr(g, x, y, w, h, r, alpha(t.bg(), k));
        if (h > 8) rr(g, x + 1, y + 1, w - 2, Math.min(h / 2, 14), Math.max(0, r - 1), alpha(t.shine(), k));
        ring(g, x, y, w, h, r, alpha(t.border(), k));
    }

    public static void bar(GuiGraphicsExtractor g, int x, int y, int w, int h, double v, int c, int back) {
        rr(g, x, y, w, h, h / 2, back);
        int f = (int) Math.round(w * Math.max(0, Math.min(1, v)));
        if (f > 0) rr(g, x, y, Math.max(f, Math.min(w, h)), h, h / 2, c);
    }

    private static final net.minecraft.network.chat.FontDescription UI = new net.minecraft.network.chat.FontDescription.Resource(net.minecraft.resources.Identifier.fromNamespaceAndPath("noxel", "ui"));
    private static final net.minecraft.network.chat.FontDescription TITLE = new net.minecraft.network.chat.FontDescription.Resource(net.minecraft.resources.Identifier.fromNamespaceAndPath("noxel", "title"));
    private static final java.util.Map<String, net.minecraft.network.chat.Component> CACHE = new java.util.HashMap<>(), CACHE_B = new java.util.HashMap<>();

    /** Гладкий шрифт вместо пиксельного: Noto Sans из ресурсов мода. */
    public static net.minecraft.network.chat.Component comp(String s, boolean bold) {
        var m = bold ? CACHE_B : CACHE;
        if (m.size() > 2000) m.clear();
        return m.computeIfAbsent(s, k -> net.minecraft.network.chat.Component.literal(k).withStyle(st -> st.withFont(bold ? TITLE : UI)));
    }

    public static int tw(String s) {
        return Minecraft.getInstance().font.width(comp(s, false));
    }

    public static void text(GuiGraphicsExtractor g, String s, int x, int y, int c, boolean shadow) {
        if (fade < 1) c = alpha(c, fade);
        g.text(Minecraft.getInstance().font, comp(s, false), x, y, c, shadow || forceShadow);
    }

    public static int twb(String s) {
        return Minecraft.getInstance().font.width(comp(s, true));
    }

    public static void btext(GuiGraphicsExtractor g, String s, int x, int y, int c) {
        if (fade < 1) c = alpha(c, fade);
        g.text(Minecraft.getInstance().font, comp(s, true), x, y, c, forceShadow);
    }

    public static void ctext(GuiGraphicsExtractor g, String s, int cx, int y, int c, boolean shadow) {
        text(g, s, cx - tw(s) / 2, y, c, shadow);
    }
}
