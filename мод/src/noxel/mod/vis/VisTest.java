package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.client.Screenshot;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.gui.screens.worldselection.CreateWorldScreen;

/** Самопроверка из лаунчера: создаёт тестовый мир, включает все модули и делает снимки HUD и меню. Работает только с -Dnoxel.visuals.selftest. */
final class VisTest {
    private VisTest() {}

    static final boolean ON = System.getProperty("noxel.visuals.selftest") != null;
    private static int step, wait, inWorld;
    private static boolean world;
    private static int dbg;

    static void tick(Minecraft mc) {
        if (!ON) return;
        if (wait > 0) { wait--; return; }
        if (mc.gui.screen() instanceof CreateWorldScreen cws) {
            for (var ch : cws.children()) {
                if (ch instanceof net.minecraft.client.gui.components.Button b && b.getMessage().getContents() instanceof net.minecraft.network.chat.contents.TranslatableContents tc && tc.getKey().equals("selectWorld.create")) {
                    System.out.println("[NoxelVis] нажимаю «Создать мир»");
                    b.onPress(new net.minecraft.client.input.KeyEvent(org.lwjgl.glfw.GLFW.GLFW_KEY_ENTER, 0, 0));
                    wait = 100;
                    return;
                }
            }
            wait = 20;
            return;
        }
        if (mc.player == null || mc.level == null) {
            if (++dbg % 50 == 0) System.out.println("[NoxelVis] экран: " + (mc.gui.screen() == null ? "нет" : mc.gui.screen().getClass().getName()));
            if (mc.gui.screen() instanceof net.minecraft.client.gui.screens.ConfirmScreen cs) {
                for (var ch : cs.children()) if (ch instanceof net.minecraft.client.gui.components.Button b) { System.out.println("[NoxelVis] подтверждаю окно"); b.onPress(new net.minecraft.client.input.KeyEvent(org.lwjgl.glfw.GLFW.GLFW_KEY_ENTER, 0, 0)); break; }
            }
            if (!world && mc.gui.screen() instanceof TitleScreen) { world = true; wait = 40; System.out.println("[NoxelVis] создаю тестовый мир"); CreateWorldScreen.testWorld(mc, () -> {}); }
            return;
        }
        if (++inWorld < 200) return;
        MenuScreen menu = mc.gui.screen() instanceof MenuScreen m ? m : null;
        switch (step++) {
            case 0 -> { Presets.apply("Всё сразу"); Waypoints.add(mc); Cfg.setEnabled("hotbar", true); Cfg.setEnabled("crosshair", true); wait = 60; }
            case 1 -> { shot(mc, "hud"); wait = 30; }
            case 2 -> { mc.setScreenAndShow(new MenuScreen(null)); wait = 30; }
            case 3 -> { shot(mc, "menu-home"); wait = 30; }
            case 4 -> { if (menu != null) menu.testTab("Информация"); wait = 40; }
            case 5 -> { shot(mc, "menu-info"); if (menu != null) menu.testDetail("radar"); wait = 40; }
            case 6 -> { shot(mc, "menu-detail"); if (menu != null) menu.testTab("Стиль"); wait = 40; }
            case 7 -> { shot(mc, "menu-style"); wait = 30; }
            case 8 -> { if (menu != null) menu.testTab("Конфиги"); wait = 30; }
            case 9 -> { shot(mc, "menu-configs"); wait = 30; }
            case 10 -> { mc.setScreenAndShow(new EditScreen(null)); wait = 30; }
            case 11 -> { shot(mc, "edit"); wait = 30; }
            case 12 -> { mc.setScreenAndShow(null); wait = 40; }
            case 13 -> { shot(mc, "hud2"); wait = 20; }
            case 14 -> { Cfg.setEnabled("viewmodel", true); Cfg.setNum("viewmodel", "x", -0.25); Cfg.setNum("viewmodel", "scale", 0.8); Cfg.setEnabled("swinganim", true); mc.getConnection().sendCommand("give @s netherite_sword"); wait = 40; }
            case 15 -> {
                for (int i = 0; i < 9; i++) if (!mc.player.getInventory().getItem(i).isEmpty()) { mc.player.getInventory().setSelectedSlot(i); break; }
                wait = 40;
            }
            case 16 -> { shot(mc, "knife-hand"); ViewModel.inspectAt = System.currentTimeMillis() - 500; wait = 10; }
            case 17 -> { shot(mc, "knife-inspect"); wait = 30; }
            case 18 -> { mc.setScreenAndShow(new net.minecraft.client.gui.screens.inventory.InventoryScreen(mc.player)); wait = 40; }
            case 19 -> { shot(mc, "knife-gui"); wait = 30; }
            default -> { System.out.println("[NoxelVis] проверка закончена"); mc.stop(); }
        }
    }

    private static void shot(Minecraft mc, String name) {
        System.out.println("[NoxelVis] снимок " + name);
        Screenshot.grab(mc, false);
    }
}
