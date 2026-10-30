package noxel;

/** Вызывает Hook.tick(); загружается тем же загрузчиком, что и GLFW. */
public final class Ticker implements Runnable {
    @Override
    public void run() {
        Hook.tick();
    }
}
