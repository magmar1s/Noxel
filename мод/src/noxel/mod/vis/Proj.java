package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.world.phys.Vec3;

/** Перевод точки мира в экранные координаты: нужно для меток, подписей над TNT и траекторий без миксинов. */
public final class Proj {
    private Proj() {}

    private static Vec3 cam = Vec3.ZERO;
    private static double yaw, pitch, tanV, aspect;
    private static int sw, sh;

    public static void begin(Minecraft mc, float partial) {
        var p = mc.player;
        if (p == null) return;
        cam = p.getEyePosition(partial);
        yaw = Math.toRadians(p.getViewYRot(partial));
        pitch = Math.toRadians(p.getViewXRot(partial));
        double fov = mc.options.fov().get();
        tanV = Math.tan(Math.toRadians(fov) / 2);
        sw = mc.getWindow().getGuiScaledWidth();
        sh = mc.getWindow().getGuiScaledHeight();
        aspect = sh == 0 ? 1 : (double) sw / sh;
    }

    /** Возвращает {x, y, расстояние} или null, если точка позади камеры. */
    public static double[] to(Vec3 w) {
        double dx = w.x - cam.x, dy = w.y - cam.y, dz = w.z - cam.z;
        double cy = Math.cos(yaw), sy = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
        // взгляд: (-sin(yaw)cos(p), -sin(p), cos(yaw)cos(p)); вправо: (-cos(yaw), 0, -sin(yaw)); вверх = вправо × взгляд
        double fx = -sy * cp, fy = -sp, fz = cy * cp;
        double rx = -cy, rz = -sy;
        double ux = -fy * rz, uy = -(fz * rx - fx * rz), uz = fy * rx;
        double z = dx * fx + dy * fy + dz * fz;
        if (z < 0.1) return null;
        double x = dx * rx + dz * rz, y = dx * ux + dy * uy + dz * uz;
        double sx = sw / 2.0 + (x / z) / (tanV * aspect) * (sw / 2.0);
        double syy = sh / 2.0 - (y / z) / tanV * (sh / 2.0);
        return new double[]{sx, syy, z};
    }
}
