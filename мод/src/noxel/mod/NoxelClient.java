package noxel.mod;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.screen.v1.ScreenEvents;
import net.fabricmc.fabric.api.client.screen.v1.Screens;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.PauseScreen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.network.chat.Component;

/** Добавляет кнопку «Друзья» в меню паузы (Esc) и на главный экран. */
public final class NoxelClient implements ClientModInitializer {
    private static boolean selfOpened;

    @Override
    public void onInitializeClient() {
        if (!Api.available()) return;                       // игра запущена не лаунчером Noxel
        System.out.println("[Noxel] мод запущен");
        ScreenEvents.AFTER_INIT.register((client, screen, w, h) -> {
            if (!(screen instanceof PauseScreen) && !(screen instanceof TitleScreen)) return;
            Screens.getWidgets(screen).add(Button.builder(Component.literal("Друзья"), b -> client.setScreenAndShow(new FriendsScreen(screen))).bounds(w - 108, 8, 100, 20).build());
            if (System.getProperty("noxel.selftest") != null && screen instanceof TitleScreen && !selfOpened) {
                selfOpened = true;                           // проверка из лаунчера: сама открывает экран друзей
                client.execute(() -> client.setScreenAndShow(new FriendsScreen(screen)));
            }
        });
    }
}
