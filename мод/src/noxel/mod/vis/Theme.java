package noxel.mod.vis;

/** Тема оформления: цвета стекла, акцент, скругление. Меняется в меню и в лаунчере. */
public record Theme(int bg, int shine, int border, int text, int sub, int accent, int radius, boolean shadow, int top, int bot, int edge1, int edge2, double spec, boolean liquid) {
    public Theme(int bg, int shine, int border, int text, int sub, int accent, int radius, boolean shadow) {
        this(bg, shine, border, text, sub, accent, radius, shadow, bg, bg, border, border, 0, false);
    }

    public static final String[] STYLES = {"Жидкое стекло", "Тёмное стекло", "Светлое стекло", "Неон", "Моно", "Прозрачное"};
    public static final String[] ACCENTS = {"Фиолетовый", "Лёд", "Мята", "Роза", "Янтарь", "Белый", "Радуга"};
    private static final int[] ACC = {0xFF9A7BFF, 0xFF6FD3FF, 0xFF5BE3A8, 0xFFFF7BB0, 0xFFFFB84D, 0xFFF2F2F5};

    public static Theme of(String style, String accent, int radius, double glass) {
        int ai = 0;
        for (int i = 0; i < ACCENTS.length; i++) if (ACCENTS[i].equals(accent)) ai = i;
        int acc = ai == 6 ? Gfx.hsv((System.currentTimeMillis() % 6000) / 6000.0, 0.55, 1) : ACC[ai];
        int ga = (int) Math.round(255 * Math.max(0.15, Math.min(1, glass)));
        return switch (style) {
            case "Жидкое стекло" -> {
                int top = Gfx.argb((int) (ga * 0.27), 0xE6EDFF), bot = Gfx.argb((int) (ga * 0.24), Gfx.mix(0x3E4C80, acc, 0.25) & 0xFFFFFF);
                yield new Theme(Gfx.mix(top, bot, .5), Gfx.argb(0, 0xFFFFFF), Gfx.argb(150, 0xFFFFFF), 0xFFFFFFFF, 0xFFE3E9FF, acc, radius, true, top, bot, Gfx.argb(215, 0xFFFFFF), Gfx.alpha(Gfx.mix(0x80FFFFFF, acc, .35), 0.55), 0.30, true);
            }
            case "Светлое стекло" -> new Theme(Gfx.argb((int) (ga * 0.8), 0xF1F1F6), Gfx.argb(60, 0xFFFFFF), Gfx.argb(120, 0xFFFFFF), 0xFF16161C, 0xFF5B5B68, acc, radius, true);
            case "Неон" -> new Theme(Gfx.argb((int) (ga * 0.72), 0x07070D), Gfx.argb(14, 0xFFFFFF), Gfx.alpha(acc, 0.85), 0xFFF4F4FF, Gfx.mix(acc, 0xFFFFFFFF, 0.35), acc, radius, true);
            case "Моно" -> new Theme(Gfx.argb((int) (ga * 0.78), 0x000000), Gfx.argb(16, 0xFFFFFF), Gfx.argb(90, 0xFFFFFF), 0xFFFFFFFF, 0xFFA0A0A8, 0xFFFFFFFF, radius, false);
            case "Прозрачное" -> new Theme(Gfx.argb((int) (ga * 0.30), 0x000000), Gfx.argb(10, 0xFFFFFF), Gfx.argb(40, 0xFFFFFF), 0xFFFFFFFF, 0xFFD0D0D8, acc, radius, false);
            default -> new Theme(Gfx.argb((int) (ga * 0.70), 0x0E0E14), Gfx.argb(18, 0xFFFFFF), Gfx.argb(46, 0xFFFFFF), 0xFFF2F2F7, 0xFFA3A3B0, acc, radius, true);
        };
    }
}
