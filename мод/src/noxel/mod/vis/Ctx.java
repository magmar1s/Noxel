package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;

/** Контекст рисования одного кадра. */
public final class Ctx {
    public final GuiGraphicsExtractor g;
    public final Theme t;
    public final Minecraft mc;
    /** true в редакторе и в меню: данных игры может не быть, рисуем образец */
    public final boolean preview;
    public final int sw, sh;

    public Ctx(GuiGraphicsExtractor g, Theme t, Minecraft mc, boolean preview) {
        this.g = g; this.t = t; this.mc = mc; this.preview = preview;
        this.sw = mc.getWindow().getGuiScaledWidth();
        this.sh = mc.getWindow().getGuiScaledHeight();
    }
}
