package noxel.mod.vis;

import java.util.ArrayDeque;
import net.minecraft.client.Minecraft;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;

/** Всё, что надо считать каждый кадр: кадры, клики, скорость, комбо, уведомления острова. */
public final class Stats {
    private Stats() {}

    public static final long START = System.currentTimeMillis();
    public static final float[] FRAMES = new float[60];
    public static int frameIdx, fps, minFps = 9999, maxFps;
    public static double fpsAvg;
    private static long lastFrame, lastFpsReset = System.currentTimeMillis();
    private static double fpsSum;
    private static int fpsN;

    public static final ArrayDeque<Long> LEFT = new ArrayDeque<>(), RIGHT = new ArrayDeque<>();
    private static boolean wasL, wasR;

    public static double speed;
    private static double px, py, pz;
    private static long pt;

    public static int combo, bestCombo;
    private static long comboAt;
    private static boolean wasHurt;

    public static String death = "";
    private static boolean wasDead;

    public static LivingEntity target;
    public static long targetAt;

    public static String toast = "";
    public static long toastAt;
    public static int toastColor;

    public static void notify(String s, int color) {
        toast = s;
        toastColor = color;
        toastAt = System.currentTimeMillis();
    }

    public static int cps(ArrayDeque<Long> q) {
        long now = System.currentTimeMillis();
        while (!q.isEmpty() && now - q.peekFirst() > 1000) q.pollFirst();
        return q.size();
    }

    public static void frame(Minecraft mc) {
        long now = System.currentTimeMillis();
        if (lastFrame != 0) {
            float ms = Math.min(100, now - lastFrame);
            FRAMES[frameIdx++ % FRAMES.length] = ms;
        }
        lastFrame = now;
        fps = mc.getFps();
        if (fps > 0) {
            fpsSum += fps; fpsN++;
            minFps = Math.min(minFps, fps); maxFps = Math.max(maxFps, fps);
        }
        if (now - lastFpsReset > 5000) {
            fpsAvg = fpsN > 0 ? fpsSum / fpsN : fps;
            fpsSum = 0; fpsN = 0; minFps = 9999; maxFps = 0; lastFpsReset = now;
        }
        if (fpsAvg == 0) fpsAvg = fps;

        boolean l = mc.options.keyAttack.isDown(), r = mc.options.keyUse.isDown();
        if (l && !wasL) LEFT.addLast(now);
        if (r && !wasR) RIGHT.addLast(now);
        wasL = l; wasR = r;

        var p = mc.player;
        if (p == null) return;
        if (pt != 0 && now - pt >= 100) {
            double dt = (now - pt) / 1000.0;
            speed = Math.hypot(p.getX() - px, p.getZ() - pz) / dt;
            px = p.getX(); py = p.getY(); pz = p.getZ(); pt = now;
        } else if (pt == 0) {
            px = p.getX(); py = p.getY(); pz = p.getZ(); pt = now;
        }

        Entity pick = mc.crosshairPickEntity;
        if (pick instanceof LivingEntity le && le != p) {
            target = le;
            targetAt = now;
            if (l && !wasHurtHit) { combo++; bestCombo = Math.max(bestCombo, combo); comboAt = now; }
        }
        wasHurtHit = l;
        boolean hurt = p.hurtTime > 0;
        if (hurt && !wasHurt) combo = 0;
        wasHurt = hurt;
        if (combo > 0 && now - comboAt > 3000) combo = 0;
        if (target != null && (target.isRemoved() || now - targetAt > 4000)) target = null;

        boolean dead = p.isDeadOrDying() || p.getHealth() <= 0;
        if (dead && !wasDead) death = String.format("%d %d %d", (int) p.getX(), (int) p.getY(), (int) p.getZ());
        wasDead = dead;
    }

    private static boolean wasHurtHit;
}
