package noxel;

import org.lwjgl.glfw.GLFW;
import org.lwjgl.glfw.GLFWCharCallback;
import org.lwjgl.glfw.GLFWCharModsCallback;
import org.lwjgl.glfw.GLFWImage;
import org.lwjgl.glfw.GLFWKeyCallback;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.RandomAccessFile;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.Base64;

/**
 * Выполняется на потоке отрисовки перед каждым опросом событий. Берёт команды из .noxel/cmd.txt и
 * «набирает» их через те же обработчики клавиатуры GLFW, что использует сама игра: клавиша чата,
 * символы, Enter. Если чат не открылся, символы игра игнорирует, поэтому пароль некуда утечь.
 */
public final class Hook {
    private Hook() {}

    private static File dir, cmdFile, ackFile, logFile;
    private static boolean ready, iconOn;
    private static long iconFirst;
    private static int iconStage;
    private static final long[] ICON_AT = {2_000_000_000L, 7_000_000_000L, 20_000_000_000L};
    private static long nextAt, offset, waitUntil;
    private static int state, chatKey = GLFW.GLFW_KEY_T, sentCount;
    private static String id, text;
    private static final ArrayDeque<String[]> queue = new ArrayDeque<String[]>();

    public static void init(String gameDir) {
        if (gameDir == null || gameDir.isEmpty()) return;
        dir = new File(gameDir, ".noxel");
        dir.mkdirs();
        cmdFile = new File(dir, "cmd.txt");
        ackFile = new File(dir, "ack.txt");
        logFile = new File(dir, "agent.log");
        offset = cmdFile.exists() ? cmdFile.length() : 0;   // старые команды не повторяем
        chatKey = readChatKey(new File(gameDir, "options.txt"));
        iconOn = readFlag(new File(dir, "flags.txt"), "icon=1");
        ready = true;
        log("агент запущен, клавиша чата " + chatKey);
    }

    static void log(String s) {
        try {
            if (logFile == null) return;
            FileOutputStream f = new FileOutputStream(logFile, true);
            f.write((s + "\n").getBytes(StandardCharsets.UTF_8));
            f.close();
        } catch (Throwable ignored) { }
    }

    private static boolean readFlag(File f, String flag) {
        try {
            if (!f.exists()) return false;
            byte[] b = new byte[(int) f.length()];
            FileInputStream in = new FileInputStream(f);
            in.read(b);
            in.close();
            return new String(b, StandardCharsets.UTF_8).contains(flag);
        } catch (Throwable t) {
            return false;
        }
    }

    private static byte[] resource(String name) throws Exception {
        InputStream in = Hook.class.getResourceAsStream(name);
        if (in == null) return null;
        ByteArrayOutputStream o = new ByteArrayOutputStream();
        byte[] buf = new byte[8192];
        for (int n; (n = in.read(buf)) > 0; ) o.write(buf, 0, n);
        in.close();
        return o.toByteArray();
    }

    /** Значок окна игры (панель задач): игра ставит свой при запуске, поэтому повторяем несколько раз. */
    private static void icon(long now) throws Exception {
        if (!iconOn || iconStage >= ICON_AT.length) return;
        long w = GLFW.glfwGetCurrentContext();
        if (w == 0L) return;
        if (iconFirst == 0L) { iconFirst = now; return; }
        if (now - iconFirst < ICON_AT[iconStage]) return;
        iconStage++;
        int[] sizes = {16, 32, 64, 128};
        ByteBuffer[] bufs = new ByteBuffer[sizes.length];
        GLFWImage.Buffer imgs = GLFWImage.malloc(sizes.length);
        try {
            for (int i = 0; i < sizes.length; i++) {
                byte[] d = resource("/noxel/icon" + sizes[i] + ".rgba");
                if (d == null) return;
                bufs[i] = ByteBuffer.allocateDirect(d.length).order(ByteOrder.nativeOrder());
                bufs[i].put(d);
                bufs[i].flip();
                imgs.get(i).set(sizes[i], sizes[i], bufs[i]);
            }
            GLFW.glfwSetWindowIcon(w, imgs);
            log("значок окна установлен");
        } finally {
            imgs.free();
        }
    }

    private static int readChatKey(File options) {
        try {
            if (!options.exists()) return GLFW.GLFW_KEY_T;
            byte[] b = new byte[(int) options.length()];
            FileInputStream in = new FileInputStream(options);
            in.read(b);
            in.close();
            for (String line : new String(b, StandardCharsets.UTF_8).split("\n")) {
                if (line.startsWith("key_key.chat:key.keyboard.")) {
                    String k = line.substring("key_key.chat:key.keyboard.".length()).trim();
                    if (k.length() == 1 && Character.isLetterOrDigit(k.charAt(0))) return Character.toUpperCase(k.charAt(0));
                    if (k.equals("slash")) return GLFW.GLFW_KEY_SLASH;
                }
            }
        } catch (Throwable ignored) { }
        return GLFW.GLFW_KEY_T;
    }

    /** Вызывается из GLFW.glfwPollEvents() на потоке отрисовки. */
    public static void tick() {
        try {
            if (!ready) return;
            long now = System.nanoTime();
            if (now < nextAt) return;
            nextAt = now + 4_000_000L;
            icon(now);
            pump(now);
        } catch (Throwable t) {
            ready = false;
            log("остановлен: " + t);
        }
    }

    private static void readNew() throws Exception {
        if (!cmdFile.exists() || cmdFile.length() <= offset) return;
        RandomAccessFile f = new RandomAccessFile(cmdFile, "r");
        f.seek(offset);
        byte[] b = new byte[(int) (f.length() - offset)];
        f.readFully(b);
        offset = f.length();
        f.close();
        for (String line : new String(b, StandardCharsets.UTF_8).split("\n")) {
            String[] p = line.trim().split("\t");
            if (p.length == 2) queue.add(new String[]{p[0], new String(Base64.getDecoder().decode(p[1]), StandardCharsets.UTF_8)});
        }
    }

    /**
     * Minecraft.getInstance().getConnection().sendCommand(команда). Имена классов читаемы только в версиях без обфускации (26.x);
     * в остальных случаях возвращает false, и команда вводится через чат.
     */
    private static boolean silent(String command) {
        try {
            ClassLoader game = Hook.class.getClassLoader().getParent();
            Class<?> mcClass = Class.forName("net.minecraft.client.Minecraft", true, game);
            Object mc = mcClass.getMethod("getInstance").invoke(null);
            Object conn = mcClass.getMethod("getConnection").invoke(mc);
            if (conn == null) return false;
            conn.getClass().getMethod("sendCommand", String.class).invoke(conn, command);
            return true;
        } catch (Throwable t) {
            if (!silentLogged) { silentLogged = true; log("тихая отправка недоступна: " + t); }
            return false;
        }
    }

    private static boolean silentLogged;

    /**
     * publish <порт>  открыть одиночный мир для сети на этом порту, без проверки лицензии у гостей (друзья без лицензии тоже зайдут)
     * unpublish       закрыть мир для сети
     * Работает в версиях с читаемыми именами классов (26.x). Возвращает null при успехе или причину отказа.
     */
    private static String control(String cmd) {
        try {
            ClassLoader game = Hook.class.getClassLoader().getParent();
            Class<?> mcClass = Class.forName("net.minecraft.client.Minecraft", true, game);
            Object mc = mcClass.getMethod("getInstance").invoke(null);
            Object srv = mcClass.getMethod("getSingleplayerServer").invoke(mc);
            if (srv == null) return "no-world";
            if (cmd.startsWith("publish ")) {
                int port = Integer.parseInt(cmd.substring(8).trim());
                srv.getClass().getMethod("setUsesAuthentication", boolean.class).invoke(srv, false);
                Class<?> scope = Class.forName("net.minecraft.server.MinecraftServer$MultiplayerScope", true, game);
                Object lan = scope.getMethod("valueOf", String.class).invoke(null, "LAN");
                Object ok = srv.getClass().getMethod("publishServer", scope, int.class).invoke(srv, lan, port);
                return Boolean.TRUE.equals(ok) ? null : "publish-failed";
            }
            if (cmd.equals("unpublish")) {
                srv.getClass().getMethod("unpublishServer").invoke(srv);
                return null;
            }
            return "unknown";
        } catch (Throwable t) {
            return "unsupported:" + t.getClass().getSimpleName();
        }
    }

    private static void ack(String what) {
        try {
            FileOutputStream f = new FileOutputStream(ackFile, true);
            f.write((id + "\t" + what + "\n").getBytes(StandardCharsets.UTF_8));
            f.close();
        } catch (Throwable ignored) { }
    }

    private static void key(long w, int key, int action) {
        GLFWKeyCallback cb = GLFW.glfwSetKeyCallback(w, null);
        GLFW.glfwSetKeyCallback(w, cb);
        if (cb != null) cb.invoke(w, key, 0, action, 0);
    }

    /** Игра принимает текст через CharMods-обработчик (в старых версиях LWJGL бывает обычный Char). */
    private static void chars(long w, String s) {
        GLFWCharModsCallback cm = GLFW.glfwSetCharModsCallback(w, null);
        GLFW.glfwSetCharModsCallback(w, cm);
        GLFWCharCallback cb = null;
        if (cm == null) {
            cb = GLFW.glfwSetCharCallback(w, null);
            GLFW.glfwSetCharCallback(w, cb);
        }
        if (cm == null && cb == null) {
            log("не найден обработчик ввода текста");
            return;
        }
        for (int i = 0; i < s.length(); ) {
            int cp = s.codePointAt(i);
            if (cm != null) cm.invoke(w, cp, 0); else cb.invoke(w, cp);
            i += Character.charCount(cp);
        }
    }

    private static final int CURSOR = 0x33001, CURSOR_NORMAL = 0x34001;
    private static long blockedSince, deadline;
    private static boolean loggedQueue;

    /** Когда открыт любой экран (чат, инвентарь, меню), игра отпускает курсор. По этому признаку видно, что чат уже готов. */
    private static boolean screenOpen(long w) {
        return GLFW.glfwGetInputMode(w, CURSOR) == CURSOR_NORMAL;
    }

    private static void pump(long now) throws Exception {
        if (state == 0) {
            readNew();
            if (queue.isEmpty()) return;
            if (!loggedQueue) { loggedQueue = true; log("в очереди команд: " + queue.size()); }
            // управляющие команды лаунчера начинаются с «!» (открыть мир для друзей и т. п.), в чат они не попадают
            String[] head = queue.peek();
            if (head != null && head[1].startsWith("!")) {
                queue.poll();
                id = head[0];
                String r = control(head[1].substring(1));
                ack(r == null ? "sent" : "error:" + r);
                log("управляющая команда «" + head[1] + "»: " + (r == null ? "выполнена" : r));
                return;
            }
            // тихий путь: команда уходит напрямую в сетевой обработчик игры, чат не открывается и клавиши не нажимаются
            if (head != null && head[1].startsWith("/") && silent(head[1].substring(1))) {
                queue.poll();
                id = head[0];
                ack("sent");
                sentCount++;
                log("команда отправлена напрямую для " + id);
                return;
            }
            long w = GLFW.glfwGetCurrentContext();
            if (w == 0L) return;
            if (screenOpen(w)) {                              // игрок в меню: ждём, чтобы не вводить в чужое окно
                if (blockedSince == 0L) { blockedSince = now; log("ждём: курсор свободен (меню или окно без фокуса), в фокусе=" + GLFW.glfwGetWindowAttrib(w, 0x20001)); }
                if (now - blockedSince > 90_000_000_000L) { String[] c = queue.poll(); id = c[0]; ack("dropped"); blockedSince = 0L; }
                return;
            }
            blockedSince = 0L;
            String[] c = queue.poll();
            id = c[0];
            text = c[1];
            key(w, chatKey, GLFW.GLFW_PRESS);
            key(w, chatKey, GLFW.GLFW_RELEASE);
            deadline = now + 4_000_000_000L;
            state = 1;
            return;
        }
        long w = GLFW.glfwGetCurrentContext();
        if (w == 0L) return;
        if (state == 1) {
            if (screenOpen(w)) {                              // чат открыт и инициализирован: вводим сразу
                chars(w, text);
                state = 2;
            } else if (now > deadline) {
                ack("chat-not-opened");
                state = 0;
                text = null;
            }
        } else {
            key(w, GLFW.GLFW_KEY_ENTER, GLFW.GLFW_PRESS);
            key(w, GLFW.GLFW_KEY_ENTER, GLFW.GLFW_RELEASE);
            ack("sent");
            sentCount++;
            state = 0;
            text = null;
        }
    }
}
