package noxel;

import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;

/**
 * Агент Noxel: даёт лаунчеру возможность «набрать» команду в чате игры (автовход, /register, /skin).
 * Работает только на LWJGL 3 (Minecraft 1.13 и новее); на старых версиях класса GLFW нет и агент ничего не делает.
 * Параметр запуска: папка игры, внутри неё лежит .noxel/cmd.txt.
 */
public final class Agent {
    private Agent() {}

    public static void premain(String args, Instrumentation inst) {
        try {
            Hook.init(args);
            inst.addTransformer(new ClassFileTransformer() {
                @Override
                public byte[] transform(ClassLoader loader, String name, Class<?> redefined, ProtectionDomain pd, byte[] bytes) {
                    if (!"org/lwjgl/glfw/GLFW".equals(name)) return null;
                    try {
                        return Patcher.patch(bytes);
                    } catch (Throwable t) {
                        Hook.log("не удалось подключиться к GLFW: " + t);
                        return null;
                    }
                }
            }, false);
        } catch (Throwable t) {
            // агент не должен мешать запуску игры
        }
    }
}
