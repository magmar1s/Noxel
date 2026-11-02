package noxel.mod.vis;

import com.google.gson.JsonArray;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.BlockHitResult;

/**
 * Помощники: умная смена инструмента и тотем в левой руке. Это автоматизация, поэтому на многих серверах она запрещена:
 * модули выключены по умолчанию, помечены предупреждением, а список «запрещённых» серверов отключает их сам.
 */
final class Assist {
    private Assist() {}

    private static int origSlot = -1;
    private static long releasedAt, lastTotem, warnedAt;
    private static String warnedFor = "";

    /** Сервер из чёрного списка пользователя: помощники там не работают совсем. */
    static boolean blocked(Minecraft mc) {
        var sd = mc.getCurrentServer();
        if (sd == null) return false;
        String ip = sd.ip.toLowerCase();
        JsonArray a = Cfg.arr("assistBlock");
        for (var e : a) {
            String b = e.getAsString().toLowerCase().trim();
            if (!b.isEmpty() && ip.contains(b)) return true;
        }
        return false;
    }

    private static boolean allowed(Minecraft mc) {
        if (!blocked(mc)) return true;
        String ip = mc.getCurrentServer().ip;
        if (!ip.equals(warnedFor) || System.currentTimeMillis() - warnedAt > 120000) {
            warnedFor = ip; warnedAt = System.currentTimeMillis();
            Stats.notify("Помощники выключены на этом сервере", 0xFFFFC14D);
        }
        return false;
    }

    static void tick(Minecraft mc) {
        var p = mc.player;
        if (p == null || mc.level == null || mc.gameMode == null) return;
        Module tool = Mods.get("autotool"), tot = Mods.get("autototem");
        boolean wantTool = tool != null && tool.on(), wantTot = tot != null && tot.on();
        if (!wantTool && !wantTot) return;
        if (!allowed(mc)) return;
        if (wantTool) tool(mc, p, tool);
        if (wantTot) totem(mc, p, tot);
    }

    private static void tool(Minecraft mc, net.minecraft.client.player.LocalPlayer p, Module m) {
        Inventory inv = p.getInventory();
        long now = System.currentTimeMillis();
        boolean breaking = mc.options.keyAttack.isDown() && mc.gui.screen() == null && mc.hitResult instanceof BlockHitResult;
        if (breaking) {
            BlockPos bp = ((BlockHitResult) mc.hitResult).getBlockPos();
            BlockState st = mc.level.getBlockState(bp);
            if (st.isAir()) return;
            int best = inv.getSelectedSlot();
            float bs = inv.getItem(best).getDestroySpeed(st);
            for (int i = 0; i < 9; i++) {
                float s = inv.getItem(i).getDestroySpeed(st);
                boolean right = inv.getItem(i).isCorrectToolForDrops(st);
                if (s > bs + 0.4f || (right && s >= bs && !inv.getItem(best).isCorrectToolForDrops(st))) { bs = s; best = i; }
            }
            if (best != inv.getSelectedSlot()) {
                if (origSlot < 0) origSlot = inv.getSelectedSlot();
                inv.setSelectedSlot(best);
            }
            releasedAt = 0;
        } else if (origSlot >= 0 && m.b("back")) {
            if (releasedAt == 0) releasedAt = now;
            if (now - releasedAt > 350) { inv.setSelectedSlot(origSlot); origSlot = -1; releasedAt = 0; }
        } else if (!m.b("back")) origSlot = -1;
    }

    private static void totem(Minecraft mc, net.minecraft.client.player.LocalPlayer p, Module m) {
        long now = System.currentTimeMillis();
        if (now - lastTotem < m.n("delay")) return;
        if (m.b("lowonly") && p.getHealth() > m.n("hpv") * 2) return;
        if (p.getOffhandItem().is(Items.TOTEM_OF_UNDYING)) return;
        if (!m.b("anyscreen") && mc.gui.screen() != null) return;
        Inventory inv = p.getInventory();
        for (int i = 0; i < 36; i++) {
            ItemStack s = inv.getItem(i);
            if (s.is(Items.TOTEM_OF_UNDYING)) {
                int slot = i < 9 ? 36 + i : i;
                mc.gameMode.handleContainerInput(p.inventoryMenu.containerId, slot, 40, ContainerInput.SWAP, p);
                lastTotem = now;
                return;
            }
        }
    }

    static void load() {
        String W = "⚠ Может быть запрещено на серверах. ";
        Mods.add(new Simple("autotool", "Умный инструмент", Mods.ASSIST, W + "Когда ломаешь блок, сам берёт нужный инструмент из хотбара: кирка для камня, топор для дерева, лопата для земли. После работы возвращает прежний предмет.", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.bool("back", "Возвращать предмет после", true)));
        Mods.add(new Simple("autototem", "Тотем в левой руке", Mods.ASSIST, W + "Следит, чтобы в левой руке всегда был тотем бессмертия: после срабатывания сам берёт следующий из инвентаря.", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.num("delay", "Пауза между действиями (мс)", 100, 1000, 50, 300)).add(Opt.bool("lowonly", "Только когда мало здоровья", false)).add(Opt.num("hpv", "Порог (сердец)", 2, 10, 1, 6)).add(Opt.bool("anyscreen", "Работать и в открытом инвентаре", false)));
    }

    static List<String> blockList() {
        List<String> out = new java.util.ArrayList<>();
        for (var e : Cfg.arr("assistBlock")) out.add(e.getAsString());
        return out;
    }
}
