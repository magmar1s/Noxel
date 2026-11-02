package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.client.Screenshot;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.gui.screens.worldselection.CreateWorldScreen;
import net.minecraft.world.InteractionHand;

/** Съёмка демо-ролика из лаунчера: создаёт мир, раздаёт ножи и по сценарию показывает доставание, взмахи и осмотр. Только с -Dnoxel.visuals.demo. */
final class DemoRunner {
    private DemoRunner() {}

    static final boolean ON = System.getProperty("noxel.visuals.demo") != null;
    private static int phase, wait, ticks, frameNo;
    private static long t0, tStart;
    private static boolean worldMade;
    private static final String[] GIVE = {"netherite_sword", "diamond_sword", "iron_sword", "golden_sword", "stone_sword"};
    private static final int LEN = 7000;

    static void tick(Minecraft mc) {
        if (!ON) return;
        if (wait > 0) { wait--; return; }
        if (mc.gui.screen() instanceof CreateWorldScreen cws) {
            for (var ch : cws.children()) {
                if (ch instanceof net.minecraft.client.gui.components.Button b && b.getMessage().getContents() instanceof net.minecraft.network.chat.contents.TranslatableContents tc && tc.getKey().equals("selectWorld.create")) {
                    b.onPress(new net.minecraft.client.input.KeyEvent(org.lwjgl.glfw.GLFW.GLFW_KEY_ENTER, 0, 0));
                    wait = 100;
                    return;
                }
            }
            wait = 20;
            return;
        }
        if (mc.gui.screen() instanceof net.minecraft.client.gui.screens.ConfirmScreen cs) {
            for (var ch : cs.children()) if (ch instanceof net.minecraft.client.gui.components.Button b) { b.onPress(new net.minecraft.client.input.KeyEvent(org.lwjgl.glfw.GLFW.GLFW_KEY_ENTER, 0, 0)); break; }
            wait = 30;
            return;
        }
        if (mc.player == null || mc.level == null) {
            if (!worldMade && mc.gui.screen() instanceof TitleScreen) { worldMade = true; wait = 40; System.out.println("[NoxelDemo] создаю мир"); CreateWorldScreen.openFresh(mc, () -> {}); }
            return;
        }
        ticks++;
        var c = mc.getConnection();
        switch (phase) {
            case 0 -> { if (ticks > 260) { phase = 1; ticks = 0; } }
            case 1 -> {
                // предметы кладём в клиентский инвентарь: в обычном мире без читов команда give не работает
                net.minecraft.world.item.Item[] items = {net.minecraft.world.item.Items.NETHERITE_SWORD, net.minecraft.world.item.Items.DIAMOND_SWORD, net.minecraft.world.item.Items.IRON_SWORD, net.minecraft.world.item.Items.GOLDEN_SWORD, net.minecraft.world.item.Items.STONE_SWORD};
                for (int k = 0; k < items.length; k++) mc.player.getInventory().setItem(k, new net.minecraft.world.item.ItemStack(items[k]));
                Presets.apply("Классика");
                for (String id : new String[]{"hotbar", "crosshair", "radar", "drawanim", "swinganim", "inspect", "viewmodel", "dmgnums"}) Cfg.setEnabled(id, true);
                Cfg.setNum("viewmodel", "scale", 0.95);
                Cfg.setNum("swinganim", "power", 1.2);
                wait = 80; phase = 2;
            }
            case 2 -> {
                mc.player.setXRot(18f);
                t0 = System.currentTimeMillis(); tStart = t0; phase = 3;
                System.out.println("[NoxelDemo] запись началась");
            }
            case 3 -> {
                long t = System.currentTimeMillis() - t0;
                int i = (int) (t / LEN), local = (int) (t % LEN);
                if (i >= GIVE.length) { System.out.println("[NoxelDemo] демо закончено кадров=" + frameNo + " мс=" + t); mc.stop(); phase = 4; return; }
                mc.player.setYRot((float) (t * 0.012));
                var inv = mc.player.getInventory();
                if (inv.getSelectedSlot() != slotOf(mc, i) && local < 400) { inv.setSelectedSlot(slotOf(mc, i)); }
                Module sw = Mods.get("swinganim"), dr = Mods.get("drawanim");
                if (sw != null) Cfg.setNum("swinganim", "style", i % 6);
                if (dr != null) Cfg.setNum("drawanim", "style", i % 4);
                for (int k = 0; k < 3; k++) if (local >= 1800 + k * 900 && local < 1800 + k * 900 + 60) mc.player.swing(InteractionHand.MAIN_HAND);
                if (local >= 4700 && local < 4760 && ViewModel.inspectAt == 0) ViewModel.inspectAt = System.currentTimeMillis();
            }
            default -> { }
        }
    }

    private static int slotOf(Minecraft mc, int i) {
        // предметы кладутся в хотбар по порядку выдачи
        return Math.min(8, i);
    }

    /** Каждый кадр во время записи: снимок экрана с порядковым номером. */
    static void frame(Minecraft mc) {
        if (!ON || phase != 3) return;
        try {
            Screenshot.grab(mc.gameDirectory, String.format("f%05d.png", frameNo++), mc.gameRenderer.mainRenderTarget(), 1, c -> { });
        } catch (Throwable t) {
            if (frameNo < 3) System.out.println("[NoxelDemo] ошибка снимка: " + t);
        }
    }
}
