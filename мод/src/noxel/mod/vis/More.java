package noxel.mod.vis;

import com.mojang.blaze3d.platform.InputConstants;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.fabricmc.fabric.api.client.message.v1.ClientReceiveMessageEvents;
import net.minecraft.ChatFormatting;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.core.particles.DustParticleOptions;
import net.minecraft.core.particles.ParticleOptions;
import net.minecraft.core.particles.ParticleTypes;
import net.minecraft.network.chat.Component;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.monster.Enemy;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import org.lwjgl.glfw.GLFW;

/** Вторая волна модулей: полезная информация, предупреждения, цифры урона, частицы, чат. */
final class More {
    private More() {}

    static KeyMapping coordsKey;

    // ── состояние ──
    private record Pick(String name, int n, long at) {}
    private record Log(String text, int color, long at) {}
    private record Float3(double x, double y, double z, String text, int color, long at) {}
    private static final List<Pick> PICKS = new ArrayList<>();
    private static final List<Log> LOG = new ArrayList<>();
    private static final List<Float3> NUMS = new ArrayList<>();
    private static final Map<String, Integer> INV = new HashMap<>();
    private static final Map<Integer, Float> HP = new HashMap<>();
    private static final int[] PINGS = new int[48];
    private static int pingIdx, kills, deaths;
    private static long lastPick, lastPing, lastMove = System.currentTimeMillis(), lastWarn;
    private static double lastX, lastY, lastZ, walked;
    private static boolean wasGround = true, wasDead, wasAttack;
    private static float myHp = -1;
    private static String lastChat = "";
    private static long lastChatAt;
    private static final Map<String, Long> WARNED = new HashMap<>();

    static void registerEvents() {
        coordsKey = KeyMappingHelper.registerKeyMapping(new KeyMapping("key.noxel.coords", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_V, Hud.cat));
        ClientReceiveMessageEvents.ALLOW_GAME.register((msg, overlay) -> {
            Module m = Mods.get("chatdedupe");
            if (overlay || m == null || !m.on()) return true;
            String s = msg.getString();
            long now = System.currentTimeMillis();
            boolean dup = s.equals(lastChat) && now - lastChatAt < m.n("sec") * 1000;
            lastChat = s; lastChatAt = now;
            return !dup;
        });
    }

    static Component modifyChat(Component msg) {
        Module hl = Mods.get("chathl"), tm = Mods.get("chattime");
        var p = Minecraft.getInstance().player;
        if (hl != null && hl.on() && p != null) {
            String me = p.getName().getString().toLowerCase(), s = msg.getString().toLowerCase();
            int i = s.indexOf(me);
            boolean mine = s.startsWith("<" + me + ">") || s.startsWith(me + ":");
            if (i >= 0 && !mine) msg = msg.copy().withStyle(ChatFormatting.GOLD, ChatFormatting.BOLD);
        }
        if (tm != null && tm.on()) {
            String t = java.time.LocalTime.now().format(java.time.format.DateTimeFormatter.ofPattern("HH:mm"));
            msg = Component.literal("[" + t + "] ").withStyle(ChatFormatting.GRAY).append(msg);
        }
        return msg;
    }

    static void tick(Minecraft mc) {
        while (coordsKey != null && coordsKey.consumeClick()) {
            Module m = Mods.get("sharecoords");
            if (m != null && m.on() && mc.gui.screen() == null && mc.player != null && mc.getConnection() != null)
                mc.getConnection().sendChat(String.format("Я здесь: %d %d %d", (int) mc.player.getX(), (int) mc.player.getY(), (int) mc.player.getZ()));
        }
    }

    private static boolean on(String id) { Module m = Mods.get(id); return m != null && m.on(); }
    private static void log(String t, int col) { LOG.add(new Log(t, col, System.currentTimeMillis())); while (LOG.size() > 12) LOG.remove(0); }

    // ── каждый кадр ──
    static void frame(Minecraft mc, Theme th) {
        var p = mc.player; var lv = mc.level;
        if (p == null || lv == null) return;
        long now = System.currentTimeMillis();
        // пинг
        if (now - lastPing > 1000) { lastPing = now; PINGS[pingIdx++ % PINGS.length] = Mods.ping(mc); }
        // активность и путь
        if (Math.abs(p.getX() - lastX) + Math.abs(p.getZ() - lastZ) > 0.02 || mc.options.keyAttack.isDown() || mc.options.keyJump.isDown()) lastMove = now;
        if (lastX != 0) walked += Math.hypot(p.getX() - lastX, p.getZ() - lastZ) < 8 ? Math.hypot(p.getX() - lastX, p.getZ() - lastZ) : 0;
        lastX = p.getX(); lastY = p.getY(); lastZ = p.getZ();
        // смерти
        boolean dead = p.getHealth() <= 0;
        if (dead && !wasDead) deaths++;
        wasDead = dead;
        // подбор предметов
        if (now - lastPick > 500) {
            lastPick = now;
            Map<String, Integer> cur = new HashMap<>();
            for (int i = 0; i < p.getInventory().getContainerSize(); i++) {
                ItemStack s = p.getInventory().getItem(i);
                if (!s.isEmpty()) cur.merge(s.getHoverName().getString(), s.getCount(), Integer::sum);
            }
            if (!INV.isEmpty() && mc.gui.screen() == null) for (var e : cur.entrySet()) {
                int d = e.getValue() - INV.getOrDefault(e.getKey(), 0);
                if (d > 0) PICKS.add(new Pick(e.getKey(), d, now));
            }
            INV.clear(); INV.putAll(cur);
            PICKS.removeIf(x -> now - x.at > 9000);
        }
        // урон: свой и по другим
        float h = p.getHealth();
        if (myHp >= 0 && h < myHp - 0.1f && on("dmgnums")) NUMS.add(new Float3(p.getX(), p.getY() + 1.9, p.getZ(), "-" + fmt(myHp - h), 0xFFFF6B6B, now));
        if (myHp >= 0 && h < myHp - 0.1f) log("Получил " + fmt(myHp - h), 0xFFFF6B6B);
        myHp = h;
        for (Entity e : lv.entitiesForRendering()) {
            if (!(e instanceof LivingEntity le) || e == p) continue;
            float prev = HP.getOrDefault(e.getId(), le.getHealth());
            if (le.getHealth() < prev - 0.1f && e.distanceTo(p) < 24) {
                if (on("dmgnums")) NUMS.add(new Float3(e.getX() + (Math.random() - .5) * .6, e.getY() + e.getBbHeight() + 0.2, e.getZ() + (Math.random() - .5) * .6, "-" + fmt(prev - le.getHealth()), 0xFFFFC14D, now));
                if (Stats.target == le) log("Ударил " + le.getName().getString() + " на " + fmt(prev - le.getHealth()), 0xFFFFC14D);
                if (le.getHealth() <= 0 && Stats.target == le) { kills++; log("Убил " + le.getName().getString(), 0xFF5BE3A8); if (on("killfx")) burst(lv, e.getX(), e.getY() + e.getBbHeight() / 2, e.getZ(), ParticleTypes.TOTEM_OF_UNDYING, 24, 0.25); }
            }
            HP.put(e.getId(), le.getHealth());
        }
        if (lv.getGameTime() % 200 == 0) HP.keySet().removeIf(id -> lv.getEntity(id) == null);
        NUMS.removeIf(n -> now - n.at > 1100);
        LOG.removeIf(l -> now - l.at > 12000);
        // частицы
        if (on("jumpring") && wasGround && !p.onGround() && p.getDeltaMovement().y > 0.2) {
            int col = th.accent() & 0xFFFFFF;
            for (int i = 0; i < 28; i++) {
                double a = i / 28.0 * Math.PI * 2;
                lv.addParticle(new DustParticleOptions(col, 1.1f), p.getX() + Math.cos(a) * .5, p.getY() + 0.05, p.getZ() + Math.sin(a) * .5, Math.cos(a) * .12, 0.02, Math.sin(a) * .12);
            }
        }
        wasGround = p.onGround();
        Module tr = Mods.get("trail");
        if (tr != null && tr.on() && Stats.speed > 0.6 && lv.getGameTime() % 1 == 0) {
            ParticleOptions po = switch (tr.ch("type")) {
                case 1 -> ParticleTypes.FLAME;
                case 2 -> ParticleTypes.HEART;
                case 3 -> ParticleTypes.NOTE;
                case 4 -> ParticleTypes.END_ROD;
                case 5 -> ParticleTypes.SOUL_FIRE_FLAME;
                default -> new DustParticleOptions(th.accent() & 0xFFFFFF, 1.0f);
            };
            lv.addParticle(po, p.getX() + (Math.random() - .5) * .3, p.getY() + 0.1, p.getZ() + (Math.random() - .5) * .3, 0, 0.01, 0);
        }
        boolean atk = mc.options.keyAttack.isDown();
        if (atk && !wasAttack && on("hiteffect") && Stats.target != null && Stats.target == mc.crosshairPickEntity) {
            Module he = Mods.get("hiteffect");
            ParticleOptions po = switch (he.ch("type")) { case 1 -> ParticleTypes.ENCHANTED_HIT; case 2 -> ParticleTypes.HEART; case 3 -> ParticleTypes.ELECTRIC_SPARK; case 4 -> ParticleTypes.FIREWORK; default -> ParticleTypes.CRIT; };
            LivingEntity t = Stats.target;
            burst(lv, t.getX(), t.getY() + t.getBbHeight() * 0.6, t.getZ(), po, (int) he.n("count"), 0.3);
        }
        wasAttack = atk;
        warnings(mc, p, now);
    }

    private static void burst(net.minecraft.client.multiplayer.ClientLevel lv, double x, double y, double z, ParticleOptions po, int n, double sp) {
        for (int i = 0; i < n; i++) lv.addParticle(po, x, y, z, (Math.random() - .5) * sp, (Math.random() - .3) * sp, (Math.random() - .5) * sp);
    }

    private static String fmt(float v) { return v == Math.floor(v) ? String.valueOf((int) v) : String.format("%.1f", v); }

    private static void warn(String key, String text, int col, long cd) {
        long now = System.currentTimeMillis();
        if (now - WARNED.getOrDefault(key, 0L) < cd) return;
        WARNED.put(key, now);
        Stats.notify(text, col);
    }

    private static void warnings(Minecraft mc, net.minecraft.client.player.LocalPlayer p, long now) {
        Module w = Mods.get("warnings");
        if (w == null || !w.on() || mc.gui.screen() != null || p.getHealth() <= 0) return;
        if (w.b("hp") && p.getHealth() <= w.n("hpv") * 2) warn("hp", "Мало здоровья!", 0xFFFF6B6B, 12000);
        if (w.b("food") && p.getFoodData().getFoodLevel() <= 6) warn("food", "Пора поесть", 0xFFFFC14D, 30000);
        if (w.b("air") && p.getAirSupply() < p.getMaxAirSupply() * 0.3 && p.isUnderWater()) warn("air", "Заканчивается воздух!", 0xFF6FD3FF, 6000);
        if (w.b("dur")) for (EquipmentSlot s : new EquipmentSlot[]{EquipmentSlot.HEAD, EquipmentSlot.CHEST, EquipmentSlot.LEGS, EquipmentSlot.FEET, EquipmentSlot.MAINHAND}) {
            ItemStack st = p.getItemBySlot(s);
            if (st.isDamageableItem() && st.getMaxDamage() > 0 && 1 - st.getDamageValue() / (double) st.getMaxDamage() < 0.1)
                warn("dur" + s, st.getHoverName().getString() + " скоро сломается", 0xFFFFC14D, 45000);
        }
        if (w.b("inv")) {
            boolean full = true;
            for (int i = 0; i < 36; i++) if (p.getInventory().getItem(i).isEmpty()) { full = false; break; }
            if (full) warn("inv", "Инвентарь заполнен", 0xFFFFC14D, 60000);
        }
    }

    // ── подписи в мире (цифры урона) ──
    static void world(Ctx c, float partial) {
        if (!on("dmgnums")) return;
        long now = System.currentTimeMillis();
        for (Float3 n : NUMS) {
            double k = (now - n.at) / 1100.0;
            double[] s = Proj.to(new Vec3(n.x, n.y + k * 0.9, n.z));
            if (s == null || s[2] > 40) continue;
            float sc = (float) Math.max(0.8, Math.min(1.8, 10 / s[2])) * (float) (1 + 0.5 * Math.max(0, 0.2 - k) * 5);
            c.g.pose().pushMatrix();
            c.g.pose().translate((float) s[0], (float) s[1]);
            c.g.pose().scale(sc, sc);
            Gfx.ctext(c.g, n.text, 0, -4, Gfx.alpha(n.color, 1 - k * k), true);
            c.g.pose().popMatrix();
        }
    }

    // ── модули ──
    static void load() {
        Mods.add(new Simple("hp", "Здоровье и голод", Mods.INFO, "Сердца, броня, голод и насыщение цифрами и полосками", false, 0.5, 0.78, (m, c) -> {
            var p = c.mc.player;
            float hp = p == null ? 17 : p.getHealth(), mx = p == null ? 20 : p.getMaxHealth(), ab = p == null ? 4 : p.getAbsorptionAmount();
            int food = p == null ? 15 : p.getFoodData().getFoodLevel(); float sat = p == null ? 4.5f : p.getFoodData().getSaturationLevel();
            int w = 110, h = m.b("food") ? 34 : 20;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            Gfx.text(c.g, "♥ " + fmt(hp) + (ab > 0 ? " +" + fmt(ab) : ""), 6, 4, hp / mx < .3 ? 0xFFFF6B6B : c.t.text(), false);
            Gfx.bar(c.g, 56, 6, w - 62, 4, hp / mx, hp / mx < .3 ? 0xFFFF6B6B : 0xFF5BE3A8, Gfx.alpha(0x000000, .4));
            if (m.b("food")) {
                Gfx.text(c.g, "◖ " + food + (m.b("sat") ? " · " + fmt(sat) : ""), 6, 21, c.t.text(), false);
                Gfx.bar(c.g, 56, 23, w - 62, 4, food / 20.0, 0xFFFFC14D, Gfx.alpha(0x000000, .4));
            }
        }).std().add(Opt.bool("food", "Показывать голод", true)).add(Opt.bool("sat", "Насыщение", true)));

        Mods.add(new Simple("xp", "Опыт", Mods.INFO, "Уровень и прогресс до следующего", false, 0.5, 0.74, (m, c) -> {
            var p = c.mc.player;
            int lvl = p == null ? 12 : p.experienceLevel; float pr = p == null ? 0.6f : p.experienceProgress;
            m.size(96, 18);
            Gfx.glass(c.g, c.t, 0, 0, 96, 18, m.alpha());
            Gfx.text(c.g, "Ур. " + lvl, 6, 5, 0xFF9AF07A, false);
            Gfx.bar(c.g, 44, 7, 46, 4, pr, 0xFF9AF07A, Gfx.alpha(0x000000, .4));
        }).std());

        Mods.add(new Simple("look", "Блок перед тобой", Mods.WORLD, "Название блока, на который смотришь, и его координаты", false, 0.5, 0.15, (m, c) -> {
            String name = "Дёрн", pos = "128 71 -40";
            if (c.mc.hitResult instanceof BlockHitResult bh && c.mc.level != null && !c.preview) {
                BlockPos bp = bh.getBlockPos();
                name = c.mc.level.getBlockState(bp).getBlock().getName().getString();
                pos = bp.getX() + " " + bp.getY() + " " + bp.getZ();
            } else if (!c.preview) { m.size(0, 0); return; }
            m.pillOut(c, name + "  ·  " + pos, c.t.accent());
        }).std());

        Mods.add(new Simple("light", "Освещённость", Mods.WORLD, "Уровень света под ногами; красный, когда на этом месте появляются мобы", false, 0.01, 0.56, (m, c) -> {
            int bl = 7, sk = 11;
            if (c.mc.level != null && c.mc.player != null && !c.preview) {
                BlockPos bp = c.mc.player.blockPosition();
                bl = c.mc.level.getBrightness(LightLayer.BLOCK, bp); sk = c.mc.level.getBrightness(LightLayer.SKY, bp);
            }
            boolean danger = bl == 0 && sk < 8;
            m.pillOut(c, "Свет: блок " + bl + " · небо " + sk + (danger ? "  ⚠ мобы" : ""), danger ? 0xFFFF6B6B : c.t.accent());
        }).std());

        Mods.add(new Simple("daycount", "День и погода", Mods.WORLD, "Номер дня, время до ночи и погода", false, 0.01, 0.62, (m, c) -> {
            long t = c.mc.level == null ? 6000 : c.mc.level.getDefaultClockTime();
            long day = t / 24000 + 1, tod = t % 24000;
            String left = tod < 12500 ? "до ночи " + (12500 - tod) / 20 / 60 + "м " + (12500 - tod) / 20 % 60 + "с" : tod < 23000 ? "ночь" : "рассвет";
            String wthr = c.mc.level == null ? "ясно" : c.mc.level.isThundering() ? "гроза" : c.mc.level.isRaining() ? "дождь" : "ясно";
            m.listOut(c, "Мир", List.of(Simple.r("День", String.valueOf(day)), Simple.r("Время", left), Simple.r("Погода", wthr)));
        }).std());

        Mods.add(new Simple("chunk", "Чанк", Mods.WORLD, "Номер чанка и позиция внутри него", false, 0.01, 0.7, (m, c) -> {
            int x = c.mc.player == null ? 100 : (int) Math.floor(c.mc.player.getX()), z = c.mc.player == null ? 40 : (int) Math.floor(c.mc.player.getZ());
            m.pillOut(c, "Чанк " + Math.floorDiv(x, 16) + ", " + Math.floorDiv(z, 16) + "  ·  " + Math.floorMod(x, 16) + ", " + Math.floorMod(z, 16), c.t.accent());
        }).std());

        Mods.add(new Simple("held", "Предмет в руке", Mods.INFO, "Название, прочность и количество предмета в руке", false, 0.5, 0.7, (m, c) -> {
            var p = c.mc.player;
            ItemStack st = p == null ? new ItemStack(Items.DIAMOND_SWORD) : p.getMainHandItem();
            if (st.isEmpty()) { m.size(0, 0); return; }
            String extra = st.isDamageableItem() ? (st.getMaxDamage() - st.getDamageValue()) + "/" + st.getMaxDamage() : st.getCount() > 1 ? "×" + st.getCount() : "";
            int w = Gfx.tw(st.getHoverName().getString()) + Gfx.tw(extra) + 40;
            m.size(w, 22);
            Gfx.glass(c.g, c.t, 0, 0, w, 22, m.alpha());
            c.g.item(st, 3, 3);
            Gfx.text(c.g, st.getHoverName().getString(), 24, 7, c.t.text(), false);
            Gfx.text(c.g, extra, w - 6 - Gfx.tw(extra), 7, c.t.sub(), false);
        }).std());

        Mods.add(new Simple("pickups", "Подобранное", Mods.INFO, "Что только что подобрал: название и количество", false, 0.01, 0.4, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            if (c.preview) { rows.add(Simple.r("Алмаз", "+3")); rows.add(Simple.r("Железный слиток", "+8")); }
            else for (int i = Math.max(0, PICKS.size() - (int) m.n("limit")); i < PICKS.size(); i++) rows.add(Simple.r(PICKS.get(i).name, "+" + PICKS.get(i).n, 0xFF5BE3A8));
            if (rows.isEmpty()) { m.size(0, 0); return; }
            m.listOut(c, "Подобрано", rows);
        }).std().add(Opt.num("limit", "Строк", 2, 10, 1, 5)));

        Mods.add(new Simple("dmglog", "Лог боя", Mods.COMBAT, "Лента: сколько урона получил, нанёс и кого убил", false, 0.01, 0.3, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            if (c.preview) { rows.add(Simple.r("Ударил Kaktus", "−4", 0xFFFFC14D)); rows.add(Simple.r("Получил", "−2.5", 0xFFFF6B6B)); }
            else for (int i = Math.max(0, LOG.size() - (int) m.n("limit")); i < LOG.size(); i++) rows.add(Simple.r(LOG.get(i).text, "", LOG.get(i).color));
            if (rows.isEmpty()) { m.size(0, 0); return; }
            m.listOut(c, "Бой", rows);
        }).std().add(Opt.num("limit", "Строк", 2, 10, 1, 5)));

        Mods.add(new Simple("cooldowns", "Перезарядки", Mods.COMBAT, "Жемчуг, хорус, щит и яблоки: сколько ждать до следующего использования", true, 0.5, 0.35, (m, c) -> {
            var p = c.mc.player;
            Object[][] items = {{Items.ENDER_PEARL, "Жемчуг"}, {Items.CHORUS_FRUIT, "Хорус"}, {Items.SHIELD, "Щит"}, {Items.GOLDEN_APPLE, "Яблоко"}, {Items.ENCHANTED_GOLDEN_APPLE, "Зач. яблоко"}, {Items.TRIDENT, "Трезубец"}, {Items.FIREWORK_ROCKET, "Фейерверк"}};
            List<Object[]> act = new ArrayList<>();
            for (Object[] it : items) {
                float v = p == null ? (it[1].equals("Жемчуг") ? 0.6f : 0f) : p.getCooldowns().getCooldownPercent(new ItemStack((net.minecraft.world.item.Item) it[0]), 0);
                if (v > 0) act.add(new Object[]{it[0], it[1], v});
            }
            if (act.isEmpty()) { m.size(0, 0); return; }
            int w = 92, h = act.size() * 17 + 6;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            int y = 3;
            for (Object[] a : act) {
                c.g.item(new ItemStack((net.minecraft.world.item.Item) a[0]), 3, y);
                Gfx.bar(c.g, 24, y + 6, w - 30, 4, 1 - (Float) a[2], c.t.accent(), Gfx.alpha(0x000000, .4));
                y += 17;
            }
        }).std());

        Mods.add(new Simple("air", "Воздух", Mods.WORLD, "Сколько секунд осталось под водой", false, 0.5, 0.68, (m, c) -> {
            var p = c.mc.player;
            if (!c.preview && (p == null || p.getAirSupply() >= p.getMaxAirSupply())) { m.size(0, 0); return; }
            int a = p == null ? 180 : p.getAirSupply(), mx = p == null ? 300 : p.getMaxAirSupply();
            m.size(86, 16);
            Gfx.glass(c.g, c.t, 0, 0, 86, 16, m.alpha());
            Gfx.bar(c.g, 6, 6, 74, 4, a / (double) mx, a < mx * .3 ? 0xFFFF6B6B : 0xFF6FD3FF, Gfx.alpha(0x000000, .4));
        }).std());

        Mods.add(new Simple("fall", "Падение", Mods.WORLD, "Сколько блоков до земли и какой урон получишь при падении", false, 0.5, 0.58, (m, c) -> {
            var p = c.mc.player;
            double dist = 0;
            if (p != null && c.mc.level != null) {
                int y = (int) Math.floor(p.getY()) - 1, x = (int) Math.floor(p.getX()), z = (int) Math.floor(p.getZ()), n = 0;
                while (n < 80 && y > c.mc.level.getMinY() && c.mc.level.getBlockState(new BlockPos(x, y, z)).getCollisionShape(c.mc.level, new BlockPos(x, y, z)).isEmpty()) { y--; n++; }
                dist = p.getY() - (y + 1) + p.fallDistance;
            } else dist = 14;
            if (!c.preview && (p == null || p.onGround() || dist < 4 || p.getDeltaMovement().y > -0.1)) { m.size(0, 0); return; }
            double dmg = Math.max(0, dist - 3);
            m.pillOut(c, String.format("До земли %.0f бл · урон %.1f♥", dist, dmg / 2), dmg >= (p == null ? 20 : p.getHealth()) ? 0xFFFF6B6B : 0xFFFFC14D);
        }).std());

        Mods.add(new Simple("elytra", "Полёт на элитрах", Mods.INFO, "Скорость, высота и прочность элитр в полёте", false, 0.5, 0.52, (m, c) -> {
            var p = c.mc.player;
            if (!c.preview && (p == null || !p.isFallFlying())) { m.size(0, 0); return; }
            ItemStack st = p == null ? new ItemStack(Items.ELYTRA) : p.getItemBySlot(EquipmentSlot.CHEST);
            String dur = st.isDamageableItem() ? (st.getMaxDamage() - st.getDamageValue()) + "" : "—";
            m.listOut(c, "Полёт", List.of(Simple.r("Скорость", String.format("%.0f бл/с", c.preview ? 28 : Stats.speed)), Simple.r("Высота", String.valueOf(p == null ? 180 : (int) p.getY())), Simple.r("Элитры", dur)));
        }).std());

        Mods.add(new Simple("mobs", "Мобы рядом", Mods.WORLD, "Сколько враждебных и мирных мобов вокруг; предупреждает, если враг близко", false, 0.01, 0.46, (m, c) -> {
            int hostile = 0, calm = 0; double near = 99;
            if (c.mc.level != null && c.mc.player != null) {
                for (Entity e : c.mc.level.entitiesForRendering()) {
                    if (e == c.mc.player || !(e instanceof LivingEntity) || e instanceof net.minecraft.world.entity.player.Player) continue;
                    double d = e.distanceTo(c.mc.player);
                    if (d > m.n("range")) continue;
                    if (e instanceof Enemy) { hostile++; near = Math.min(near, d); } else calm++;
                }
            } else { hostile = 3; calm = 5; near = 6; }
            m.listOut(c, "Мобы", List.of(Simple.r("Враждебные", String.valueOf(hostile), near < 8 ? 0xFFFF6B6B : c.t.text()), Simple.r("Мирные", String.valueOf(calm))));
        }).std().add(Opt.num("range", "Дальность", 8, 64, 4, 32)));

        Mods.add(new Simple("session", "Сессия", Mods.INFO, "Убийства, смерти и пройденный путь за эту игру", false, 0.01, 0.84, (m, c) -> m.listOut(c, "Сессия", List.of(
            Simple.r("Убито", String.valueOf(c.preview ? 6 : kills)), Simple.r("Смертей", String.valueOf(c.preview ? 1 : deaths)), Simple.r("Пройдено", (int) (c.preview ? 1250 : walked) + " бл")))).std());

        Mods.add(new Simple("afk", "AFK-таймер", Mods.MISC, "Показывает, сколько ты ничего не делаешь (после минуты)", false, 0.5, 0.3, (m, c) -> {
            long s = (System.currentTimeMillis() - lastMove) / 1000;
            if (!c.preview && s < m.n("after")) { m.size(0, 0); return; }
            m.pillOut(c, "AFK " + (c.preview ? "1:24" : s / 60 + ":" + String.format("%02d", s % 60)), 0xFFFFC14D);
        }).std().add(Opt.num("after", "Показывать после (с)", 10, 300, 10, 60)));

        Mods.add(new Simple("pinggraph", "График пинга", Mods.INFO, "Как менялся пинг за последнюю минуту", false, 0.01, 0.94, (m, c) -> {
            int w = 100, h = 36;
            m.size(w, h);
            Gfx.glass(c.g, c.t, 0, 0, w, h, m.alpha());
            int mx = 40;
            for (int v : PINGS) mx = Math.max(mx, v);
            for (int i = 0; i < PINGS.length; i++) {
                int v = c.preview ? 20 + (int) (15 * Math.sin(i / 4.0)) + 15 : PINGS[(pingIdx + i) % PINGS.length];
                int bh = Math.max(1, (int) (v / (double) mx * 22));
                c.g.fill(4 + i * 2, 30 - bh, 5 + i * 2, 30, Gfx.alpha(v > 150 ? 0xFFFF6B6B : c.t.accent(), 0.9));
            }
            Gfx.text(c.g, "пинг " + (c.preview ? 28 : Mods.ping(c.mc)) + " мс", 5, 2, c.t.sub(), false);
        }).std());

        Mods.add(new Simple("keybinds", "Клавиши Noxel", Mods.MISC, "Список горячих клавиш визуалов: меню, метка, приближение", false, 0.99, 0.2, (m, c) -> {
            List<String[]> rows = new ArrayList<>();
            rows.add(Simple.r("Меню", keyName(Hud.menuKey)));
            rows.add(Simple.r("Метка", keyName(Hud.wpKey)));
            rows.add(Simple.r("Приближение", keyName(Hud.zoomKey)));
            if (coordsKey != null) rows.add(Simple.r("Координаты в чат", keyName(coordsKey)));
            m.listOut(c, "Клавиши", rows);
        }).std());

        Mods.add(new Simple("warnings", "Предупреждения", Mods.INFO, "Всплывающие подсказки в острове: мало здоровья, голод, воздух, сломается предмет, инвентарь полон", true, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.bool("hp", "Мало здоровья", true)).add(Opt.num("hpv", "Порог (сердец)", 1, 8, 1, 3)).add(Opt.bool("food", "Голод", true)).add(Opt.bool("air", "Воздух", true)).add(Opt.bool("dur", "Прочность предметов", true)).add(Opt.bool("inv", "Инвентарь полон", false)));
        Mods.add(new Simple("sharecoords", "Координаты в чат", Mods.MISC, "Клавиша V отправляет в чат твои координаты", false, 0, 0, (m, c) -> {}).noPanel());
        Mods.add(new Simple("dmgnums", "Цифры урона", Mods.COMBAT, "Всплывающие числа урона над целью и над тобой", true, 0, 0, (m, c) -> {}).noPanel());
        Mods.add(new Simple("jumpring", "Круги от прыжка", Mods.SCREEN, "Светящееся кольцо частиц цветом акцента при каждом прыжке", false, 0, 0, (m, c) -> {}).noPanel());
        Mods.add(new Simple("trail", "След", Mods.SCREEN, "Красивый след из частиц за тобой при беге", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.choice("type", "Вид", 0, "Цвет акцента", "Пламя", "Сердца", "Ноты", "Звёзды", "Синий огонь")));
        Mods.add(new Simple("hiteffect", "Эффект удара", Mods.SCREEN, "Вспышка частиц на цели при ударе", false, 0, 0, (m, c) -> {}).noPanel()
            .add(Opt.choice("type", "Вид", 0, "Крит", "Зачарование", "Сердца", "Искры", "Салют")).add(Opt.num("count", "Сколько частиц", 4, 40, 2, 14)));
        Mods.add(new Simple("killfx", "Эффект убийства", Mods.SCREEN, "Салют из частиц, когда добиваешь цель", false, 0, 0, (m, c) -> {}).noPanel());
        Mods.add(new Simple("chathl", "Подсветка ника в чате", Mods.MISC, "Сообщения, где упомянули тебя, выделяются золотым жирным", false, 0, 0, (m, c) -> {}).noPanel());
        Mods.add(new Simple("chatdedupe", "Антиспам в чате", Mods.MISC, "Прячет одинаковые сообщения подряд", false, 0, 0, (m, c) -> {}).noPanel().add(Opt.num("sec", "Окно (секунд)", 2, 30, 1, 6)));
    }

    private static String keyName(KeyMapping k) {
        try { return k == null ? "—" : KeyMappingHelper.getBoundKeyOf(k).getDisplayName().getString(); } catch (Throwable t) { return "—"; }
    }
}
