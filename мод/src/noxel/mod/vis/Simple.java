package noxel.mod.vis;

import java.util.function.BiConsumer;
import net.minecraft.client.Minecraft;

/** Модуль, описанный лямбдой рисования. */
public final class Simple extends Module {
    private final BiConsumer<Simple, Ctx> fn;
    private java.util.function.Consumer<Minecraft> frame;

    public Simple(String id, String name, String cat, String desc, boolean defOn, double dx, double dy, BiConsumer<Simple, Ctx> fn) {
        super(id, name, cat, desc, defOn, dx, dy);
        this.fn = fn;
    }

    public Simple add(Opt o) { opt(o); return this; }
    public Simple frameFn(java.util.function.Consumer<Minecraft> f) { frame = f; return this; }
    public Simple noPanel() { panel = false; return this; }
    public Simple std() { common(); return this; }

    @Override public void frame(Minecraft mc) { if (frame != null) frame.accept(mc); }
    @Override public void draw(Ctx c) { fn.accept(this, c); }
    // доступ к защищённым помощникам из лямбд
    public void listOut(Ctx c, String title, java.util.List<String[]> rows) { list(c, title, rows); }
    public void pillOut(Ctx c, String text, int dot) { pill(c, text, dot); }
    public void size(int w, int h) { this.w = w; this.h = h; }
    public static String[] r(String a, String b) { return row(a, b); }
    public static String[] r(String a, String b, int col) { return row(a, b, col); }
}
