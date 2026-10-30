package noxel;

import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;

/**
 * Подключает вызов Hook.tick() к GLFW.glfwPollEvents() без библиотек для работы с байт-кодом:
 * исходный метод переименовывается в glfwPollEvents$noxel, а рядом добавляется новый glfwPollEvents(),
 * который вызывает Hook.tick() и затем исходный метод. Новый метод без ветвлений, поэтому StackMapTable не нужен.
 */
final class Patcher {
    private Patcher() {}

    static byte[] patch(byte[] in) throws Exception {
        ByteBuffer b = ByteBuffer.wrap(in);
        b.getInt(); b.getShort(); b.getShort();
        int cpCount = b.getShort() & 0xFFFF;
        Map<Integer, String> utf8 = new HashMap<Integer, String>();
        for (int i = 1; i < cpCount; i++) {
            int tag = b.get() & 0xFF;
            switch (tag) {
                case 1: { int len = b.getShort() & 0xFFFF; byte[] s = new byte[len]; b.get(s); utf8.put(i, new String(s, StandardCharsets.UTF_8)); break; }
                case 3: case 4: case 9: case 10: case 11: case 12: case 17: case 18: b.position(b.position() + 4); break;
                case 5: case 6: b.position(b.position() + 8); i++; break;
                case 7: case 8: case 16: case 19: case 20: b.position(b.position() + 2); break;
                case 15: b.position(b.position() + 3); break;
                default: throw new IllegalStateException("тег константы " + tag);
            }
        }
        int cpEnd = b.position();
        b.getShort();
        int thisClass = b.getShort() & 0xFFFF;
        b.getShort();
        int itf = b.getShort() & 0xFFFF;
        b.position(b.position() + itf * 2);
        for (int k = 0; k < 2; k++) {               // поля, затем методы
            int n = b.getShort() & 0xFFFF;
            if (k == 1) return finish(in, b, cpCount, cpEnd, thisClass, utf8, n);
            for (int i = 0; i < n; i++) { b.position(b.position() + 6); skipAttrs(b); }
        }
        return null;
    }

    private static void skipAttrs(ByteBuffer b) {
        int n = b.getShort() & 0xFFFF;
        for (int i = 0; i < n; i++) { b.getShort(); int len = b.getInt(); b.position(b.position() + len); }
    }

    private static byte[] finish(byte[] in, ByteBuffer b, int cpCount, int cpEnd, int thisClass, Map<Integer, String> utf8, int methods) throws Exception {
        int methodsStart = b.position();
        int nameIdxPos = -1, nameIdx = -1, descIdx = -1;
        for (int i = 0; i < methods; i++) {
            int at = b.position();
            b.getShort();
            int name = b.getShort() & 0xFFFF, desc = b.getShort() & 0xFFFF;
            if ("glfwPollEvents".equals(utf8.get(name)) && "()V".equals(utf8.get(desc))) { nameIdxPos = at + 2; nameIdx = name; descIdx = desc; }
            skipAttrs(b);
        }
        int methodsEnd = b.position();
        if (nameIdxPos < 0) return null;

        int base = cpCount;                          // номера новых констант
        int A = base, B = base + 1, C = base + 2, D = base + 3, F = base + 4, G = base + 5, H = base + 6, I = base + 7, K = base + 8;
        ByteArrayOutputStream cp = new ByteArrayOutputStream();
        DataOutputStream o = new DataOutputStream(cp);
        utf(o, "glfwPollEvents$noxel");              // A
        utf(o, "noxel/Hook");                        // B
        o.writeByte(7); o.writeShort(B);             // C: класс Hook
        utf(o, "tick");                              // D
        o.writeByte(12); o.writeShort(D); o.writeShort(descIdx);        // F: tick()V
        o.writeByte(10); o.writeShort(C); o.writeShort(F);              // G: Hook.tick()V
        utf(o, "Code");                              // H
        o.writeByte(12); o.writeShort(A); o.writeShort(descIdx);        // I: glfwPollEvents$noxel()V
        o.writeByte(10); o.writeShort(thisClass); o.writeShort(I);      // K: GLFW.glfwPollEvents$noxel()V
        o.flush();

        ByteArrayOutputStream out = new ByteArrayOutputStream(in.length + 128);
        DataOutputStream w = new DataOutputStream(out);
        w.write(in, 0, 8);
        w.writeShort(cpCount + 9);
        w.write(in, 10, cpEnd - 10);
        w.write(cp.toByteArray());
        // от access_flags до числа методов
        w.write(in, cpEnd, methodsStart - 2 - cpEnd);
        w.writeShort(methods + 1);
        byte[] ms = new byte[methodsEnd - methodsStart];
        System.arraycopy(in, methodsStart, ms, 0, ms.length);
        int rel = nameIdxPos - methodsStart;
        ms[rel] = (byte) (A >> 8); ms[rel + 1] = (byte) A;          // исходный метод получает новое имя
        w.write(ms);
        // новый метод: public static glfwPollEvents() { Hook.tick(); glfwPollEvents$noxel(); }
        w.writeShort(0x0009); w.writeShort(nameIdx); w.writeShort(descIdx);
        w.writeShort(1); w.writeShort(H);
        w.writeInt(2 + 2 + 4 + 7 + 2 + 2);
        w.writeShort(0); w.writeShort(0);
        w.writeInt(7);
        w.writeByte(0xB8); w.writeShort(G);
        w.writeByte(0xB8); w.writeShort(K);
        w.writeByte(0xB1);
        w.writeShort(0); w.writeShort(0);
        w.write(in, methodsEnd, in.length - methodsEnd);             // атрибуты класса
        w.flush();
        return out.toByteArray();
    }

    private static void utf(DataOutputStream o, String s) throws Exception {
        byte[] v = s.getBytes(StandardCharsets.UTF_8);
        o.writeByte(1); o.writeShort(v.length); o.write(v);
    }
}
