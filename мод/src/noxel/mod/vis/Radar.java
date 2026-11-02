package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.levelgen.Heightmap;

/** Радар: круглая карта вокруг игрока с рельефом и точками существ. */
final class Radar {
    private Radar() {}

    private static final int G = 30;
    private static final int[] GRID = new int[G * G];
    private static long built;
    private static double bx, bz, byaw;

    static void draw(Simple m, Ctx c) {
        Minecraft mc = c.mc;
        int cell = Math.max(2, (int) m.n("size") / G), d = G * cell, R = d / 2;
        m.size(d + 6, d + 6);
        Gfx.glass(c.g, c.t, 0, 0, d + 6, d + 6, m.alpha());
        int ox = 3, oy = 3;
        var p = mc.player;
        double range = m.n("range");
        double yaw = p == null ? 0 : Math.toRadians(p.getYRot());
        boolean rot = m.b("rotate");
        double fx = rot ? -Math.sin(yaw) : 0, fz = rot ? Math.cos(yaw) : -1, rx = rot ? -Math.cos(yaw) : 1, rz = rot ? -Math.sin(yaw) : 0;

        if (m.b("terrain") && p != null && mc.level != null) {
            long now = System.currentTimeMillis();
            if (now - built > 800 || Math.hypot(p.getX() - bx, p.getZ() - bz) > 2 || Math.abs(yaw - byaw) > 0.15) {
                built = now; bx = p.getX(); bz = p.getZ(); byaw = yaw;
                for (int j = 0; j < G; j++) for (int i = 0; i < G; i++) {
                    double sx = (i + 0.5 - G / 2.0) / (G / 2.0) * range, sy = (G / 2.0 - j - 0.5) / (G / 2.0) * range;
                    int wx = (int) Math.floor(p.getX() + sx * rx + sy * fx), wz = (int) Math.floor(p.getZ() + sx * rz + sy * fz);
                    GRID[j * G + i] = colorAt(mc, wx, wz);
                }
            }
            for (int j = 0; j < G; j++) {
                int run = 0;
                for (int i = 0; i <= G; i++) {
                    int col = i < G ? GRID[j * G + i] : 0;
                    int prev = i > 0 ? GRID[j * G + i - 1] : 0;
                    boolean inside = i < G && inCircle(i, j);
                    if (i > 0 && (!inside || col != prev) && run > 0) {
                        c.g.fill(ox + (i - run) * cell, oy + j * cell, ox + i * cell, oy + (j + 1) * cell, Gfx.alpha(prev | 0xFF000000, 0.78 * m.alpha()));
                        run = 0;
                    }
                    if (inside) run++;
                }
            }
        } else {
            for (int j = 0; j < G; j++) for (int i = 0; i < G; i++) if (inCircle(i, j)) c.g.fill(ox + i * cell, oy + j * cell, ox + (i + 1) * cell, oy + (j + 1) * cell, Gfx.alpha(c.t.sub(), 0.12));
        }
        // кольцо и крест
        circle(c, ox + R, oy + R, R, Gfx.alpha(c.t.border(), m.alpha()));
        c.g.fill(ox + R, oy + 1, ox + R + 1, oy + d - 1, Gfx.alpha(0xFFFFFF, 0.08));
        c.g.fill(ox + 1, oy + R, ox + d - 1, oy + R + 1, Gfx.alpha(0xFFFFFF, 0.08));
        Gfx.ctext(c.g, "С", ox + R, oy + 1, Gfx.alpha(c.t.accent(), 1), false);

        if (p != null && mc.level != null) {
            for (Entity e : mc.level.entitiesForRendering()) {
                if (e == p) continue;
                int col;
                if (e instanceof Player && m.b("players")) col = 0xFFFF6B6B;
                else if (e instanceof Mob && m.b("mobs")) col = 0xFFFFC14D;
                else if (e instanceof ItemEntity && m.b("items")) col = 0xFF6FD3FF;
                else continue;
                double dx = e.getX() - p.getX(), dz = e.getZ() - p.getZ();
                double sx = (dx * rx + dz * rz) / range * R, sy = -(dx * fx + dz * fz) / range * R;
                if (Math.hypot(sx, sy) > R - 2) { double k = (R - 2) / Math.hypot(sx, sy); sx *= k; sy *= k; }
                int px = ox + R + (int) Math.round(sx), py = oy + R + (int) Math.round(sy);
                c.g.fill(px - 1, py - 1, px + 2, py + 2, 0xFF000000);
                c.g.fill(px, py, px + 1, py + 1, col);
                c.g.fill(px - 1, py - 1, px + 2, py + 2, Gfx.alpha(col, 0.95));
            }
        } else if (c.preview) {
            c.g.fill(ox + R + 12, oy + R - 9, ox + R + 15, oy + R - 6, 0xFFFF6B6B);
            c.g.fill(ox + R - 14, oy + R + 8, ox + R - 11, oy + R + 11, 0xFFFFC14D);
        }
        // стрелка игрока: острие по направлению взгляда
        int cx = ox + R, cy = oy + R;
        double vx = rot || p == null ? 0 : -Math.sin(yaw), vy = rot || p == null ? -1 : Math.cos(yaw);
        c.g.fill(cx - 2, cy - 2, cx + 3, cy + 3, Gfx.alpha(0xFF101015, 0.9));
        for (int k = 0; k <= 4; k++) {
            int tx = cx + (int) Math.round(vx * k), ty = cy + (int) Math.round(vy * k);
            c.g.fill(tx, ty, tx + 1, ty + 1, c.t.accent());
        }
        c.g.fill(cx, cy, cx + 1, cy + 1, 0xFFFFFFFF);
    }

    private static boolean inCircle(int i, int j) {
        double dx = i + 0.5 - G / 2.0, dy = j + 0.5 - G / 2.0;
        return dx * dx + dy * dy <= (G / 2.0) * (G / 2.0);
    }

    private static void circle(Ctx c, int cx, int cy, int r, int col) {
        for (int a = 0; a < 360; a += 3) {
            int x = cx + (int) Math.round(Math.cos(Math.toRadians(a)) * r), y = cy + (int) Math.round(Math.sin(Math.toRadians(a)) * r);
            c.g.fill(x, y, x + 1, y + 1, col);
        }
    }

    private static int colorAt(Minecraft mc, int x, int z) {
        try {
            int h = mc.level.getHeight(Heightmap.Types.WORLD_SURFACE, x, z);
            BlockPos pos = new BlockPos(x, h - 1, z);
            var st = mc.level.getBlockState(pos);
            int col = st.getMapColor(mc.level, pos).col;
            if (col == 0) col = 0x303038;
            int shade = (int) Math.max(-24, Math.min(24, (h - mc.player.getY()) * 1.2));
            int r = Math.max(0, Math.min(255, ((col >> 16) & 255) + shade)), g = Math.max(0, Math.min(255, ((col >> 8) & 255) + shade)), b = Math.max(0, Math.min(255, (col & 255) + shade));
            return (r << 16) | (g << 8) | b;
        } catch (Throwable t) {
            return 0x202026;
        }
    }
}
