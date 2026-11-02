package noxel.mod.vis;

import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.input.KeyEvent;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.network.chat.Component;
import org.lwjgl.glfw.GLFW;

/** Редактор расположения: перетаскивай панели мышью, колесо — размер, R — сброс положения, Del — выключить. */
public final class EditScreen extends Screen {
    private final Screen parent;
    private Module drag, hover;
    private int offX, offY;

    public EditScreen(Screen parent) {
        super(Component.literal("Расположение"));
        this.parent = parent;
    }

    @Override public boolean isPauseScreen() { return false; }
    @Override public void onClose() { Cfg.save(); minecraft.setScreenAndShow(parent); }

    @Override
    public void extractRenderState(GuiGraphicsExtractor g, int mx, int my, float delta) {
        Cfg.poll();
        Theme t = Hud.theme();
        g.fill(0, 0, width, height, 0x55000000);
        // сетка и центральные линии
        g.fill(width / 2, 0, width / 2 + 1, height, Gfx.alpha(t.accent(), 0.18));
        g.fill(0, height / 2, width, height / 2 + 1, Gfx.alpha(t.accent(), 0.18));
        Ctx c = new Ctx(g, t, minecraft, true);
        Hud.layout(c);
        hover = null;
        for (Module m : Mods.ALL) {
            if (!m.panel || !m.on()) continue;
            Hud.place(c, m, true);
            int[] r = Hud.rect(c, m);
            if (r[2] <= 0) continue;
            boolean in = mx >= r[0] && mx < r[0] + r[2] && my >= r[1] && my < r[1] + r[3];
            if (in) hover = m;
            Gfx.ring(g, r[0] - 2, r[1] - 2, r[2] + 4, r[3] + 4, 4, Gfx.alpha(m == drag || m == hover ? t.accent() : 0xFFFFFF, m == drag || m == hover ? 0.95 : 0.25));
        }
        Gfx.ctext(g, "Тяни панели мышью  ·  колесо — размер  ·  R — сбросить  ·  Del — выключить  ·  Esc — готово", width / 2, height - 14, t.text(), true);
        if (hover != null) Gfx.ctext(g, hover.name, width / 2, 8, t.accent(), true);
    }

    @Override
    public boolean mouseClicked(MouseButtonEvent e, boolean dbl) {
        if (hover != null) {
            Ctx c = new Ctx(null, Hud.theme(), minecraft, true);
            Hud.layout(c);
            int[] r = Hud.rect(c, hover);
            drag = hover; offX = (int) e.x() - r[0]; offY = (int) e.y() - r[1];
            return true;
        }
        return super.mouseClicked(e, dbl);
    }

    @Override
    public boolean mouseDragged(MouseButtonEvent e, double dx, double dy) {
        if (drag == null) return super.mouseDragged(e, dx, dy);
        Ctx c = new Ctx(null, Hud.theme(), minecraft, true);
        int[] r = Hud.rect(c, drag);
        double room = Math.max(1, c.sw - r[2]), roomY = Math.max(1, c.sh - r[3]);
        double fx = Math.max(0, Math.min(1, (e.x() - offX) / room)), fy = Math.max(0, Math.min(1, (e.y() - offY) / roomY));
        if (Math.abs(fx - 0.5) < 0.012) fx = 0.5;
        if (Math.abs(fy - 0.5) < 0.012) fy = 0.5;
        Cfg.setNum(drag.id, "x", fx);
        Cfg.setNum(drag.id, "y", fy);
        return true;
    }

    @Override public boolean mouseReleased(MouseButtonEvent e) { drag = null; return super.mouseReleased(e); }

    @Override
    public boolean mouseScrolled(double mx, double my, double sx, double sy) {
        if (hover != null) { Cfg.setNum(hover.id, "scale", Math.max(0.6, Math.min(2, Math.round((hover.scale() + sy * 0.1) * 10) / 10.0))); return true; }
        return false;
    }

    @Override
    public boolean keyPressed(KeyEvent e) {
        if (hover != null && e.key() == GLFW.GLFW_KEY_R) { Cfg.unset(hover.id, "x"); Cfg.unset(hover.id, "y"); return true; }
        if (hover != null && e.key() == GLFW.GLFW_KEY_DELETE) { Cfg.setEnabled(hover.id, false); return true; }
        return super.keyPressed(e);
    }
}
