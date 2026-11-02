package noxel.mod.vis;

import net.minecraft.client.Minecraft;
import net.minecraft.client.resources.sounds.SimpleSoundInstance;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;

/** Звуки Noxel: нарисованы кодом (сталь, взмах, щелчки), лежат в самом моде. */
public final class Snd {
    private Snd() {}

    public static void play(String name, float pitch, float vol) {
        try {
            SoundEvent ev = SoundEvent.createVariableRangeEvent(Identifier.fromNamespaceAndPath("noxel", name));
            Minecraft.getInstance().getSoundManager().play(SimpleSoundInstance.forUI(ev, pitch, vol));
        } catch (Throwable ignored) {
        }
    }

    public static void ui(float vol) { play("ui.tick", 1f + (float) (Math.random() * .12), vol); }
}
