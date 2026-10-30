package noxel;

import org.lwjgl.glfw.GLFW;
import org.lwjgl.glfw.GLFWCharCallback;
import org.lwjgl.glfw.GLFWKeyCallback;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.RandomAccessFile;
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
    private static boolean ready;
    private static long nextAt, offset, waitUntil;
    private static int state, chatKey = GLFW.GLFW_KEY_T;
    private static String id, text;
    private static final ArrayDeque<String[]> queue = new ArrayDeque<String[]>();

    static void init(String gameDir) {
        if (gameDir == null || gameDir.isEmpty()) return;
        dir = new File(gameDir, ".noxel");
        dir.mkdirs();
        cmdFile = new File(dir, "cmd.txt");
        ackFile = new File(dir, "ack.txt");
        logFile = new File(dir, "agent.log");
        offset = cmdFile.exists() ? cmdFile.length() : 0;   // старые команды не повторяем
        chatKey = readChatKey(new File(gameDir, "options.txt"));
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
            nextAt = now + 30_000_000L;
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

    private static void chars(long w, String s) {
        GLFWCharCallback cb = GLFW.glfwSetCharCallback(w, null);
        GLFW.glfwSetCharCallback(w, cb);
        if (cb == null) return;
        for (int i = 0; i < s.length(); ) {
            int cp = s.codePointAt(i);
            cb.invoke(w, cp);
            i += Character.charCount(cp);
        }
    }

    private static void pump(long now) throws Exception {
        if (state == 0) {
            readNew();
            if (queue.isEmpty()) return;
            long w = GLFW.glfwGetCurrentContext();
            if (w == 0L) return;
            String[] c = queue.poll();
            id = c[0];
            text = c[1];
            key(w, chatKey, GLFW.GLFW_PRESS);
            key(w, chatKey, GLFW.GLFW_RELEASE);
            state = 1;
            waitUntil = now + 350_000_000L;                 // игра открывает чат на следующем тике
            return;
        }
        if (now < waitUntil) return;
        long w = GLFW.glfwGetCurrentContext();
        if (w == 0L) return;
        if (state == 1) {
            chars(w, text);
            state = 2;
            waitUntil = now + 150_000_000L;
        } else {
            key(w, GLFW.GLFW_KEY_ENTER, GLFW.GLFW_PRESS);
            key(w, GLFW.GLFW_KEY_ENTER, GLFW.GLFW_RELEASE);
            ack("sent");
            state = 0;
            text = null;
        }
    }
}
