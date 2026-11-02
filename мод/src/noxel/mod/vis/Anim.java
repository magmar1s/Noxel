package noxel.mod.vis;

import java.util.HashMap;
import java.util.Map;

/** Плавные переходы для меню: наведение, переключатели, прокрутка, смена вкладок (как в хорошем веб-интерфейсе). */
public final class Anim {
    private Anim() {}

    private static final Map<String, float[]> M = new HashMap<>();
    private static long last = System.nanoTime();
    private static float dt = 0.016f;

    /** Раз в кадр в начале отрисовки меню. */
    public static void begin() {
        long n = System.nanoTime();
        dt = Math.max(0.001f, Math.min(0.05f, (n - last) / 1e9f));
        last = n;
        if (M.size() > 600) M.clear();
    }

    /** Плавно ведёт значение к цели; speed — «жёсткость» (10–20 приятно). */
    public static float to(String key, float target, float speed) {
        float[] v = M.computeIfAbsent(key, k -> new float[]{target});
        v[0] += (target - v[0]) * Math.min(1f, speed * dt);
        if (Math.abs(target - v[0]) < 0.002f) v[0] = target;
        return v[0];
    }

    public static float dt() { return dt; }

    public static double ease(double t) { t = Math.max(0, Math.min(1, t)); return 1 - Math.pow(1 - t, 3); }
}
