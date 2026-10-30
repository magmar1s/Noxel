package noxel;

import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.net.URL;
import java.net.URLClassLoader;
import java.security.ProtectionDomain;

/**
 * Агент Noxel: даёт лаунчеру возможность «набрать» команду в чате игры (автовход, /register, /skin).
 * Работает только на LWJGL 3 (Minecraft 1.13 и новее); на старых версиях класса GLFW нет и агент ничего не делает.
 * Параметр запуска: папка игры, внутри неё лежит .noxel/cmd.txt.
 *
 * Загрузчики классов игры (Fabric Knot) не видят классы агента, поэтому GLFW вызывает не Hook напрямую, а
 * Runnable из System.getProperties(). Hook и Ticker грузятся отдельным загрузчиком, чей родитель — загрузчик GLFW.
 */
public final class Agent {
    private Agent() {}

    private static final String KEY = "noxel.tick";

    private static final class Starter implements Runnable {
        private final ClassLoader game;
        private final URL jar;
        private final String args;

        Starter(ClassLoader game, URL jar, String args) {
            this.game = game;
            this.jar = jar;
            this.args = args;
        }

        @Override
        public void run() {
            Runnable next = new Runnable() { @Override public void run() { } };
            try {
                URLClassLoader cl = new URLClassLoader(new URL[]{jar}, game) {
                    @Override
                    protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                        if (!name.startsWith("noxel.")) return super.loadClass(name, resolve);
                        synchronized (getClassLoadingLock(name)) {
                            Class<?> c = findLoadedClass(name);
                            if (c == null) c = findClass(name);
                            if (resolve) resolveClass(c);
                            return c;
                        }
                    }
                };
                cl.loadClass("noxel.Hook").getMethod("init", String.class).invoke(null, args);
                next = (Runnable) cl.loadClass("noxel.Ticker").getDeclaredConstructor().newInstance();
            } catch (Throwable t) {
                // агент не должен мешать запуску игры
            }
            System.getProperties().put(KEY, next);
            next.run();
        }
    }

    public static void premain(final String args, Instrumentation inst) {
        try {
            final URL jar = Agent.class.getProtectionDomain().getCodeSource().getLocation();
            System.getProperties().put(KEY, new Runnable() { @Override public void run() { } });
            inst.addTransformer(new ClassFileTransformer() {
                @Override
                public byte[] transform(ClassLoader loader, String name, Class<?> redefined, ProtectionDomain pd, byte[] bytes) {
                    if (!"org/lwjgl/glfw/GLFW".equals(name)) return null;
                    try {
                        byte[] r = Patcher.patch(bytes);
                        if (r != null) System.getProperties().put(KEY, new Starter(loader, jar, args));
                        return r;
                    } catch (Throwable t) {
                        return null;
                    }
                }
            }, false);
        } catch (Throwable t) {
            // агент не должен мешать запуску игры
        }
    }
}
