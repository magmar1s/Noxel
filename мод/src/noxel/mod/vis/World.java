package noxel.mod.vis;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.projectile.Projectile;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.phys.Vec3;

/** Подписи и траектории прямо в мире: рисуются слоем HUD через собственную проекцию. */
final class World {
    private World() {}

    static void draw(Ctx c, float partial) {
        Minecraft mc = c.mc;
        if (mc.player == null || mc.level == null || mc.gui.screen() != null && !(mc.gui.screen() instanceof EditScreen)) return;
        Proj.begin(mc, partial);
        Module tnt = Mods.get("tntworld"), wp = Mods.get("wpworld"), pr = Mods.get("predict"), hp = Mods.get("tags");
        if (tnt != null && tnt.on()) tntLabels(c, tnt, partial);
        if (wp != null && wp.on()) waypointMarks(c, wp);
        if (pr != null && pr.on()) predict(c, pr, partial);
        if (hp != null && hp.on()) tags(c, hp, partial);
        Module st = Mods.get("storage");
        if (st != null && st.on() && st.b("world")) Storage.marks(c, st);
    }

    private static void label(Ctx c, double[] s, String text, int col, double scale) {
        float k = (float) scale;
        c.g.pose().pushMatrix();
        c.g.pose().translate((float) s[0], (float) s[1]);
        c.g.pose().scale(k, k);
        int w = Gfx.tw(text) + 8;
        Gfx.rr(c.g, -w / 2, -6, w, 13, 4, Gfx.alpha(c.t.bg(), 0.9));
        Gfx.ring(c.g, -w / 2, -6, w, 13, 4, Gfx.alpha(col, 0.8));
        Gfx.text(c.g, text, -w / 2 + 4, -3, col, false);
        c.g.pose().popMatrix();
    }

    private static void tntLabels(Ctx c, Module m, float partial) {
        for (Entity e : c.mc.level.entitiesForRendering()) {
            if (!(e instanceof PrimedTnt t)) continue;
            double[] s = Proj.to(t.getPosition(partial).add(0, 1.2, 0));
            if (s == null || s[2] > 48) continue;
            double sec = t.getFuse() / 20.0;
            label(c, s, String.format("%.1f", sec), sec < 1.5 ? 0xFFFF6B6B : 0xFFFFC14D, Math.max(0.7, Math.min(1.4, 8 / s[2])));
        }
    }

    private static void waypointMarks(Ctx c, Module m) {
        Minecraft mc = c.mc;
        for (var e : Cfg.arr("waypoints")) {
            var o = e.getAsJsonObject();
            if (!o.get("k").getAsString().equals(Waypoints.key(mc)) || !o.get("d").getAsString().equals(Waypoints.dim(mc))) continue;
            Vec3 p = new Vec3(o.get("x").getAsDouble() + 0.5, o.get("y").getAsDouble() + 1.2, o.get("z").getAsDouble() + 0.5);
            double[] s = Proj.to(p);
            if (s == null) continue;
            int sx = (int) Math.max(14, Math.min(c.sw - 14, s[0])), sy = (int) Math.max(14, Math.min(c.sh - 14, s[1]));
            double d = mc.player.position().distanceTo(p);
            c.g.fill(sx - 1, sy - 9, sx + 2, sy - 6, c.t.accent());
            label(c, new double[]{sx, sy, s[2]}, o.get("n").getAsString() + " · " + (int) d + " м", c.t.accent(), 0.85);
        }
    }

    private static void tags(Ctx c, Module m, float partial) {
        Minecraft mc = c.mc;
        for (Player p : mc.level.players()) {
            if (p == mc.player) continue;
            double[] s = Proj.to(p.getPosition(partial).add(0, p.getBbHeight() + 0.55, 0));
            if (s == null || s[2] > m.n("range")) continue;
            int w = 54, x = (int) s[0] - w / 2, y = (int) s[1];
            double v = Math.max(0, Math.min(1, p.getHealth() / p.getMaxHealth()));
            Gfx.rr(c.g, x - 2, y - 4, w + 4, 10, 4, Gfx.alpha(c.t.bg(), 0.85));
            Gfx.bar(c.g, x, y, w, 3, v, v < 0.3 ? 0xFFFF6B6B : v < 0.6 ? 0xFFFFC14D : 0xFF5BE3A8, Gfx.alpha(0x000000, 0.4));
            if (m.b("armor") && p.getArmorValue() > 0) Gfx.bar(c.g, x, y + 4, w, 1, p.getArmorValue() / 20.0, 0xFF6FD3FF, 0);
        }
    }

    // ── траектории ──
    private static void predict(Ctx c, Module m, float partial) {
        Minecraft mc = c.mc;
        var p = mc.player;
        ItemStack st = p.getMainHandItem();
        double speed = 0, grav = 0.03, drag = 0.99;
        if (m.b("hand")) {
            if (st.is(Items.ENDER_PEARL) || st.is(Items.SNOWBALL) || st.is(Items.EGG)) { speed = 1.5; }
            else if (st.is(Items.SPLASH_POTION) || st.is(Items.LINGERING_POTION)) { speed = 0.5; grav = 0.05; }
            else if (st.is(Items.EXPERIENCE_BOTTLE)) { speed = 0.7; grav = 0.07; }
            else if (st.is(Items.TRIDENT)) { speed = 2.5; grav = 0.05; }
            else if ((st.is(Items.BOW) || st.is(Items.CROSSBOW)) && (p.isUsingItem() || st.is(Items.CROSSBOW))) {
                double pull = st.is(Items.CROSSBOW) ? 1 : Math.min(1, (p.getTicksUsingItem()) / 20.0);
                pull = (pull * pull + pull * 2) / 3;
                speed = 3.0 * pull; grav = 0.05;
            }
        }
        if (speed > 0.05) {
            double yaw = Math.toRadians(p.getViewYRot(partial)), pit = Math.toRadians(p.getViewXRot(partial));
            Vec3 dir = new Vec3(-Math.sin(yaw) * Math.cos(pit), -Math.sin(pit), Math.cos(yaw) * Math.cos(pit)).scale(speed);
            Vec3 pos = p.getEyePosition(partial).add(-Math.cos(yaw) * 0.16, -0.1, -Math.sin(yaw) * 0.16);
            trace(c, m, pos, dir, grav, drag, true, 0);
        }
        if (m.b("flying")) {
            for (Entity e : mc.level.entitiesForRendering()) {
                if (!(e instanceof Projectile pr) || e.isRemoved()) continue;
                Vec3 v = e.getDeltaMovement();
                if (v.lengthSqr() < 0.01) continue;
                double g = e.getType().toString().contains("arrow") || e.getType().toString().contains("potion") ? 0.05 : 0.03;
                trace(c, m, e.position(), v, g, 0.99, false, 0);
            }
        }
    }

    private static void trace(Ctx c, Module m, Vec3 pos, Vec3 vel, double grav, double drag, boolean mine, int dummy) {
        Minecraft mc = c.mc;
        List<double[]> pts = new ArrayList<>();
        Vec3 land = null;
        int ticks = 0;
        for (int i = 0; i < 160; i++) {
            Vec3 next = pos.add(vel);
            // шаги по 0.25 блока, чтобы не пролететь сквозь стену
            double len = next.subtract(pos).length();
            int steps = Math.max(1, (int) Math.ceil(len / 0.25));
            boolean hit = false;
            for (int k = 1; k <= steps; k++) {
                Vec3 q = pos.add(next.subtract(pos).scale(k / (double) steps));
                if (!mc.level.getBlockState(BlockPos.containing(q)).getCollisionShape(mc.level, BlockPos.containing(q)).isEmpty()) { land = pos.add(next.subtract(pos).scale((k - 1) / (double) steps)); hit = true; break; }
            }
            if (hit) { ticks = i + 1; break; }
            pos = next;
            vel = vel.scale(drag).add(0, -grav, 0);
            if (i % 2 == 0) { double[] s = Proj.to(pos); if (s != null) pts.add(s); }
            if (pos.y < mc.level.getMinY() - 8) break;
        }
        int col = mine ? c.t.accent() : 0xFFFFC14D;
        for (int i = 0; i < pts.size(); i++) {
            double[] s = pts.get(i);
            c.g.fill((int) s[0], (int) s[1], (int) s[0] + 2, (int) s[1] + 2, Gfx.alpha(col, 0.35 + 0.65 * i / Math.max(1, pts.size())));
        }
        if (land != null) {
            double[] s = Proj.to(land);
            if (s != null) {
                c.g.fill((int) s[0] - 3, (int) s[1] - 3, (int) s[0] + 4, (int) s[1] + 4, Gfx.alpha(col, 0.9));
                c.g.fill((int) s[0] - 2, (int) s[1] - 2, (int) s[0] + 3, (int) s[1] + 3, Gfx.alpha(c.t.bg(), 1));
                if (m.b("time")) Gfx.ctext(c.g, String.format("%.1f с", ticks / 20.0), (int) s[0], (int) s[1] + 6, col, true);
            }
        }
    }
}
