package noxel.mod.vis;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.math.Axis;
import net.minecraft.client.player.AbstractClientPlayer;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.ItemStack;

/** Рука и оружие от первого лица: сдвиг, размер, свои взмахи, доставание и «осмотр» на клавишу. */
public final class ViewModel {
    private ViewModel() {}

    public static long inspectAt;
    private static ItemStack lastStack;
    private static long drawAt;
    private static float lastSwing;

    private static double ease(double t) { t = Math.max(0, Math.min(1, t)); return t < .5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; }

    public static void apply(AbstractClientPlayer p, InteractionHand hand, float swing, float equip, ItemStack stack, PoseStack ps) {
        if (hand != InteractionHand.MAIN_HAND) return;
        Module vm = Mods.get("viewmodel"), sw = Mods.get("swinganim"), dr = Mods.get("drawanim"), in = Mods.get("inspect");
        long now = System.currentTimeMillis();
        if (lastStack == null || !ItemStack.isSameItem(stack, lastStack)) {
            lastStack = stack.copy(); drawAt = now;
            if (dr != null && dr.on() && dr.b("sound") && !stack.isEmpty()) Snd.play("knife.draw", 0.95f + (float) Math.random() * 0.15f, (float) dr.n("vol"));
        }
        if (sw != null && sw.on() && swing > 0 && lastSwing == 0 && sw.b("sound")) Snd.play("knife.swing", 0.9f + (float) Math.random() * 0.25f, (float) sw.n("vol"));
        lastSwing = swing;
        if (vm != null && vm.on()) {
            ps.translate((float) vm.n("x"), (float) vm.n("y"), (float) vm.n("z"));
            float s = (float) vm.n("scale");
            ps.scale(s, s, s);
            ps.mulPose(Axis.ZP.rotationDegrees((float) vm.n("roll")));
        }
        if (sw != null && sw.on() && swing > 0) {
            double a = Math.sin(swing * Math.PI), k = sw.n("power");
            switch (sw.ch("style")) {
                case 1 -> { ps.translate(0, 0, (float) (-0.55 * a * k)); ps.mulPose(Axis.XP.rotationDegrees((float) (-10 * a * k))); }          // колющий
                case 2 -> { ps.mulPose(Axis.YP.rotationDegrees((float) (-70 * a * k))); ps.translate((float) (0.3 * a * k), 0, 0); }                // широкий взмах
                case 3 -> ps.mulPose(Axis.ZP.rotationDegrees((float) (360 * ease(swing) * k)));                                                    // вращение
                case 4 -> { ps.translate(0, (float) (0.25 * a * k), (float) (-0.2 * a * k)); ps.mulPose(Axis.XP.rotationDegrees((float) (-55 * a * k))); } // сверху вниз
                case 5 -> ps.translate((float) (Math.sin(swing * Math.PI * 4) * 0.08 * k), (float) (-0.1 * a * k), 0);                              // дрожь
                default -> { ps.mulPose(Axis.ZP.rotationDegrees((float) (-35 * a * k))); ps.translate(0, 0, (float) (-0.15 * a * k)); }            // рубящий
            }
        }
        if (dr != null && dr.on()) {
            double t = (now - drawAt) / (dr.n("ms"));
            if (t < 1) {
                double e = 1 - ease(t);
                switch (dr.ch("style")) {
                    case 1 -> ps.mulPose(Axis.YP.rotationDegrees((float) (360 * e)));                                       // крутится
                    case 2 -> { ps.translate((float) (0.9 * e), (float) (-0.3 * e), 0); ps.mulPose(Axis.ZP.rotationDegrees((float) (-80 * e))); } // вылетает сбоку
                    case 3 -> { ps.translate(0, (float) (-0.8 * e), 0); ps.mulPose(Axis.XP.rotationDegrees((float) (90 * e))); }                  // поднимается снизу
                    default -> ps.mulPose(Axis.XP.rotationDegrees((float) (-120 * e)));                                   // откидывается
                }
            }
        }
        if (in != null && in.on() && inspectAt > 0) {
            double t = (now - inspectAt) / in.n("ms");
            if (t >= 1) inspectAt = 0;
            else {
                double e = Math.sin(Math.min(1, t * 1.15) * Math.PI);        // поднял, повертел и вернул
                ps.translate((float) (-0.25 * e), (float) (0.15 * e), (float) (-0.1 * e));
                ps.mulPose(Axis.YP.rotationDegrees((float) (30 * e + 360 * ease(t))));
                ps.mulPose(Axis.XP.rotationDegrees((float) (-25 * e)));
                ps.mulPose(Axis.ZP.rotationDegrees((float) (40 * Math.sin(t * Math.PI * 2))));
            }
        }
    }

    static void load() {
        Mods.add(new Simple("viewmodel", "Положение рук", Mods.SCREEN, "Сдвигает и уменьшает руку и предмет от первого лица: убери меч с глаз или подвинь куда удобно", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.num("x", "Влево / вправо", -0.6, 0.6, 0.05, 0)).add(Opt.num("y", "Вниз / вверх", -0.6, 0.6, 0.05, 0)).add(Opt.num("z", "Ближе / дальше", -0.8, 0.8, 0.05, 0))
            .add(Opt.num("scale", "Размер", 0.4, 1.4, 0.05, 1)).add(Opt.num("roll", "Наклон", -45, 45, 5, 0)));
        Mods.add(new Simple("swinganim", "Анимация удара", Mods.SCREEN, "Свой взмах оружием: рубящий, колющий, широкий, вращение, сверху вниз, дрожь", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.choice("style", "Стиль", 0, "Рубящий", "Колющий", "Широкий взмах", "Вращение", "Сверху вниз", "Дрожь")).add(Opt.num("power", "Сила", 0.3, 2, 0.1, 1)).add(Opt.bool("sound", "Звук взмаха", true)).add(Opt.num("vol", "Громкость", 0.1, 1, 0.1, 0.6)));
        Mods.add(new Simple("drawanim", "Анимация доставания", Mods.SCREEN, "Красивое появление предмета в руке при смене слота", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.choice("style", "Стиль", 0, "Откидывается", "Крутится", "Вылетает сбоку", "Поднимается снизу")).add(Opt.num("ms", "Длительность (мс)", 200, 1200, 50, 550)).add(Opt.bool("sound", "Звук доставания (шух)", true)).add(Opt.num("vol", "Громкость", 0.1, 1, 0.1, 0.7)));
        Mods.add(new Simple("inspect", "Осмотр предмета", Mods.SCREEN, "Клавиша G: повертеть предмет в руке перед собой, как нож в играх", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.num("ms", "Длительность (мс)", 800, 4000, 100, 1800)).add(Opt.bool("sound", "Щелчки при осмотре", true)).add(Opt.num("vol", "Громкость", 0.1, 1, 0.1, 0.7)));
    }
}
