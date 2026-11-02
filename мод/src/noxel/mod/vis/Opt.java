package noxel.mod.vis;

/** Настройка модуля: переключатель, число или выбор из списка. */
public final class Opt {
    public enum Kind { BOOL, NUM, CHOICE }

    public final String key, name;
    public final Kind kind;
    public final double min, max, step, def;
    public final String[] choices;

    private Opt(String key, String name, Kind kind, double min, double max, double step, double def, String[] choices) {
        this.key = key; this.name = name; this.kind = kind; this.min = min; this.max = max; this.step = step; this.def = def; this.choices = choices;
    }

    public static Opt bool(String key, String name, boolean def) {
        return new Opt(key, name, Kind.BOOL, 0, 1, 1, def ? 1 : 0, null);
    }

    public static Opt num(String key, String name, double min, double max, double step, double def) {
        return new Opt(key, name, Kind.NUM, min, max, step, def, null);
    }

    public static Opt choice(String key, String name, int def, String... choices) {
        return new Opt(key, name, Kind.CHOICE, 0, choices.length - 1, 1, def, choices);
    }
}
