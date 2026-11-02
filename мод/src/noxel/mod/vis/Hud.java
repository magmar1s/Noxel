package noxel.mod.vis;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.fabricmc.fabric.api.client.message.v1.ClientReceiveMessageEvents;
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElement;
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry;
import net.fabricmc.fabric.api.client.rendering.v1.hud.VanillaHudElements;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.world.item.ItemStack;
import org.lwjgl.glfw.GLFW;

/** Подключение визуалов к игре: рисование слоя HUD, замена прицела и хотбара, клавиши. */
public final class Hud {
    private Hud() {}

    static KeyMapping menuKey, wpKey, zoomKey, inspectKey;
    static KeyMapping.Category cat;
    private static boolean zoomOn;
    private static int zoomSaved = -1;
    private static double zoomLevel = 1;
    private static boolean ready;

    public static Theme theme() {
        Theme t = Theme.of(Cfg.gs("style", "Жидкое стекло"), Cfg.gs("accent", "Фиолетовый"), (int) Cfg.gn("radius", 7), Cfg.gn("glass", 0.9));
        Gfx.forceShadow = t.liquid();
        return t;
    }

    public static void init() {
        Cfg.load();
        cat = KeyMapping.Category.register(Identifier.fromNamespaceAndPath("noxel", "visuals"));
        menuKey = KeyMappingHelper.registerKeyMapping(new KeyMapping("key.noxel.menu", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_RIGHT_SHIFT, cat));
        wpKey = KeyMappingHelper.registerKeyMapping(new KeyMapping("key.noxel.waypoint", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_B, cat));
        inspectKey = KeyMappingHelper.registerKeyMapping(new KeyMapping("key.noxel.inspect", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_G, cat));
        zoomKey = KeyMappingHelper.registerKeyMapping(new KeyMapping("key.noxel.zoom", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_C, cat));

        HudElementRegistry.addLast(Identifier.fromNamespaceAndPath("noxel", "visuals"), Hud::render);
        HudElementRegistry.replaceElement(VanillaHudElements.CROSSHAIR, prev -> (g, dt) -> {
            if (!crosshair(g)) prev.extractRenderState(g, dt);
        });
        HudElementRegistry.replaceElement(VanillaHudElements.HOTBAR, prev -> (g, dt) -> {
            if (!hotbar(g)) prev.extractRenderState(g, dt);
        });
        ClientTickEvents.END_CLIENT_TICK.register(Hud::tick);
        More.registerEvents();
        ClientReceiveMessageEvents.MODIFY_GAME.register((msg, overlay) -> overlay ? msg : More.modifyChat(msg));
        ready = true;
    }

    private static void tick(Minecraft mc) {
        if (!ready) return;
        while (menuKey.consumeClick()) if (mc.gui.screen() == null) mc.setScreenAndShow(new MenuScreen(null));
        while (wpKey.consumeClick()) if (mc.gui.screen() == null) Waypoints.add(mc);
        while (inspectKey.consumeClick()) if (mc.gui.screen() == null) { ViewModel.inspectAt = System.currentTimeMillis(); Module im = Mods.get("inspect"); if (im != null && im.on() && im.b("sound")) { Snd.play("knife.flip", 1f, (float) im.n("vol")); } }
        Storage.tick(mc);
        More.tick(mc);
        Assist.tick(mc);
        VisTest.tick(mc);
        zoomTick(mc);
        if (mc.player != null && mc.player.getHealth() <= 0) { /* точку смерти пишет Stats */ }
    }

    // ── приближение ──
    private static void zoomTick(Minecraft mc) {
        Module z = Mods.get("zoom");
        boolean want = z != null && z.on() && mc.gui.screen() == null && zoomKey.isDown();
        var opt = mc.options.fov();
        try {
            if (want && !zoomOn) { zoomSaved = opt.get(); zoomOn = true; zoomLevel = 1; }
            if (zoomOn) {
                int target = want ? (int) Math.max(10, Math.min(opt.get(), z.n("fov") * zoomLevel)) : zoomSaved;
                int cur = opt.get();
                int next = z != null && z.b("smooth") ? cur + (int) Math.signum(target - cur) * Math.max(1, Math.min(Math.abs(target - cur), Math.abs(target - cur) / 3 + 1)) : target;
                setFov(opt, next);
                if (!want && next == zoomSaved) { zoomOn = false; }
            }
        } catch (Throwable t) {
            zoomOn = false;
        }
    }

    private static void setFov(net.minecraft.client.OptionInstance<Integer> opt, int v) throws Exception {
        // set() сверяет значение с допустимым диапазоном ползунка (30–110); для зума нужно меньше, поэтому пишем поле напрямую
        java.lang.reflect.Field f = net.minecraft.client.OptionInstance.class.getDeclaredField("value");
        f.setAccessible(true);
        f.set(opt, v);
    }

    public static boolean zoomScroll(double dy) {
        if (!zoomOn) return false;
        zoomLevel = Math.max(0.4, Math.min(2.2, zoomLevel - dy * 0.12));
        return true;
    }

    // ── прицел ──
    private static boolean crosshair(GuiGraphicsExtractor g) {
        Module m = Mods.get("crosshair");
        Minecraft mc = Minecraft.getInstance();
        if (m == null || !m.on() || mc.gui.screen() != null && !(mc.gui.screen() instanceof MenuScreen) || !mc.options.getCameraType().isFirstPerson()) return m != null && m.on();
        Ctx c = new Ctx(g, theme(), mc, false);
        drawCrosshair(c, m, c.sw / 2, c.sh / 2);
        return true;
    }

    static void drawCrosshair(Ctx c, Module m, int cx, int cy) {
        int shape = m.ch("shape"), size = (int) m.n("size"), gap = (int) m.n("gap"), th = (int) m.n("thick");
        var p = c.mc.player;
        if (m.b("dyn") && p != null && (p.isSprinting() || Stats.speed > 3.5)) gap += 3;
        int col = switch (m.ch("color")) {
            case 1 -> 0xFFFFFFFF;
            case 2 -> 0xFFFF5555;
            case 3 -> 0xFF55FF77;
            case 4 -> Gfx.hsv((System.currentTimeMillis() % 4000) / 4000.0, 0.7, 1);
            default -> c.t.accent();
        };
        boolean ol = m.b("outline");
        var g = c.g;
        switch (shape) {
            case 0 -> { if (ol) g.fill(cx - th - 1, cy - th - 1, cx + th + 2, cy + th + 2, 0xCC000000); g.fill(cx - th + 1 - 1, cy - th + 1 - 1, cx + th, cy + th, col); }
            case 2 -> {
                int r = size + gap;
                for (int a = 0; a < 360; a += 4) {
                    int x = cx + (int) Math.round(Math.cos(Math.toRadians(a)) * r), y = cy + (int) Math.round(Math.sin(Math.toRadians(a)) * r);
                    if (ol) g.fill(x - 1, y - 1, x + th + 1, y + th + 1, 0x99000000);
                }
                for (int a = 0; a < 360; a += 4) {
                    int x = cx + (int) Math.round(Math.cos(Math.toRadians(a)) * r), y = cy + (int) Math.round(Math.sin(Math.toRadians(a)) * r);
                    g.fill(x, y, x + th, y + th, col);
                }
            }
            case 3 -> {
                int r = size + gap;
                if (ol) g.outline(cx - r - 1, cy - r - 1, r * 2 + 3, r * 2 + 3, 0xCC000000);
                g.outline(cx - r, cy - r, r * 2 + 1, r * 2 + 1, col);
            }
            default -> {
                int a = gap, b = gap + size;
                int[][] arms = {{-b, -th / 2, -a, th - th / 2}, {a, -th / 2, b, th - th / 2}, {-th / 2, -b, th - th / 2, -a}, {-th / 2, a, th - th / 2, b}};
                if (ol) for (int[] r : arms) g.fill(cx + r[0] - 1, cy + r[1] - 1, cx + r[2] + 1, cy + r[3] + 1, 0xCC000000);
                for (int[] r : arms) g.fill(cx + r[0], cy + r[1], cx + r[2], cy + r[3], col);
            }
        }
    }

    // ── хотбар ──
    private static boolean hotbar(GuiGraphicsExtractor g) {
        Module m = Mods.get("hotbar");
        Minecraft mc = Minecraft.getInstance();
        if (m == null || !m.on() || mc.player == null) return false;
        Ctx c = new Ctx(g, theme(), mc, false);
        int cell = 20, w = 9 * cell + 6, x = (c.sw - w) / 2, y = c.sh - 26;
        Gfx.glass(g, c.t, x, y, w, 24, 1);
        int sel = mc.player.getInventory().getSelectedSlot();
        for (int i = 0; i < 9; i++) {
            int sx = x + 3 + i * cell;
            if (i == sel) {
                Gfx.rr(g, sx, y + 2, cell, 20, 6, Gfx.alpha(c.t.accent(), 0.35));
                Gfx.ring(g, sx, y + 2, cell, 20, 6, c.t.accent());
            }
            ItemStack st = mc.player.getInventory().getItem(i);
            if (st.isEmpty()) continue;
            g.item(st, sx + 2, y + 4);
            g.itemDecorations(mc.font, st, sx + 2, y + 4);
        }
        ItemStack off = mc.player.getOffhandItem();
        if (!off.isEmpty()) {
            Gfx.glass(g, c.t, x - 26, y, 22, 24, 1);
            g.item(off, x - 24, y + 4);
            g.itemDecorations(mc.font, off, x - 24, y + 4);
        }
        return true;
    }

    // ── основной слой ──
    private static void render(GuiGraphicsExtractor g, net.minecraft.client.DeltaTracker dt) {
        Minecraft mc = Minecraft.getInstance();
        if (!ready || mc.player == null) return;
        Cfg.poll();
        Stats.frame(mc);
        Ctx c = new Ctx(g, theme(), mc, false);
        for (Module m : Mods.ALL) m.frame(mc);
        More.frame(mc, c.t);
        vignette(c);
        if (mc.gui.screen() instanceof MenuScreen || mc.gui.screen() instanceof EditScreen) return;
        drawAll(c, false);
        World.draw(c, dt.getGameTimeDeltaPartialTick(false));
        More.world(c, dt.getGameTimeDeltaPartialTick(false));
    }

    private static final java.util.Map<Module, int[]> AUTO = new java.util.HashMap<>();

    /** Расстановка без наложений: панели, которые пользователь не двигал, встают столбиками у краёв экрана. */
    static void layout(Ctx c) {
        AUTO.clear();
        int left = 4, right = 4, top = 22, bottom = 44;
        for (Module m : Mods.ALL) {
            if (!m.panel || !m.on() || Cfg.has(m.id, "x") || m.w <= 0 || m.h <= 0) continue;
            double s = m.scale();
            int w = (int) Math.round(m.w * s), h = (int) Math.round(m.h * s);
            if (m.dx < 0.34) { AUTO.put(m, new int[]{4, left, w, h}); left += h + 4; }
            else if (m.dx > 0.66) { AUTO.put(m, new int[]{c.sw - w - 4, right, w, h}); right += h + 4; }
            else if (m.dy < 0.5) {
                int ty = m.id.equals("island") ? 3 : top;
                AUTO.put(m, new int[]{(c.sw - w) / 2, ty, w, h});
                if (!m.id.equals("island")) top += h + 4; else top = Math.max(top, 3 + h + 4);
            } else { bottom += h + 4; AUTO.put(m, new int[]{(c.sw - w) / 2, c.sh - bottom - 20, w, h}); }
        }
    }

    /** Рисует все включённые панели на их местах. */
    static void drawAll(Ctx c, boolean editor) {
        layout(c);
        for (Module m : Mods.ALL) {
            if (!m.panel || !m.on()) continue;
            place(c, m, editor);
        }
    }

    static int[] rect(Ctx c, Module m) {
        double s = m.scale();
        int w = (int) Math.round(m.w * s), h = (int) Math.round(m.h * s);
        int[] a = AUTO.get(m);
        if (a != null && !Cfg.has(m.id, "x")) return new int[]{a[0], a[1], w, h};
        int x = (int) Math.round(m.fx() * (c.sw - w)), y = (int) Math.round(m.fy() * (c.sh - h));
        return new int[]{x, y, w, h};
    }

    static void place(Ctx c, Module m, boolean editor) {
        int[] r = rect(c, m);
        c.g.pose().pushMatrix();
        c.g.pose().translate(r[0], r[1]);
        float s = (float) m.scale();
        c.g.pose().scale(s, s);
        try {
            m.draw(c);
        } catch (Throwable t) {
            m.w = 0; m.h = 0;
        }
        c.g.pose().popMatrix();
    }

    private static void vignette(Ctx c) {
        Module m = Mods.get("vignette");
        var p = c.mc.player;
        if (m == null || !m.on() || p == null || p.getHealth() <= 0) return;
        double hp = p.getHealth() / 2.0;
        if (hp > m.n("hp")) return;
        double k = 0.35 + 0.25 * Math.sin(System.currentTimeMillis() / 220.0);
        int a = (int) (110 * k * (1.5 - hp / Math.max(1, m.n("hp"))));
        int w = c.sw, h = c.sh, e = Math.max(24, h / 6);
        int col = Gfx.argb(a, 0xFF2030), none = Gfx.argb(0, 0xFF2030);
        c.g.fillGradient(0, 0, w, e, col, none);
        c.g.fillGradient(0, h - e, w, h, none, col);
    }
}
