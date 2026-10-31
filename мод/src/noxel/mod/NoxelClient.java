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
    private static boolean selfOpened, selfSwitched;

    @Override
    public void onInitializeClient() {
        if (!Api.available()) return;                       // игра запущена не лаунчером Noxel
        System.out.println("[Noxel] мод запущен");
        ScreenEvents.AFTER_INIT.register((client, screen, w, h) -> {
            if (!(screen instanceof PauseScreen) && !(screen instanceof TitleScreen)) return;
            Screens.getWidgets(screen).add(Button.builder(Component.literal("Друзья"), b -> client.setScreenAndShow(new FriendsScreen(screen))).bounds(w - 108, 8, 100, 20).build());
            String who = client.getUser().getName();
            Screens.getWidgets(screen).add(Button.builder(Component.literal("Аккаунт: " + (who.length() > 9 ? who.substring(0, 8) + "…" : who)), b -> client.setScreenAndShow(new AccountScreen(screen))).bounds(w - 108, 32, 100, 20).build());
            String sw = System.getProperty("noxel.selftest.switch");
            if (sw != null && !selfSwitched) {
                selfSwitched = true;                         // проверка из лаунчера: сама выбирает другой аккаунт и меняет имя в игре
                java.util.concurrent.CompletableFuture.runAsync(() -> {
                    try {
                        var list = Api.call("GET", "/accounts", null).getAsJsonArray("accounts");
                        for (var e : list) {
                            var a = e.getAsJsonObject();
                            if (a.get("name").getAsString().equals(sw)) {
                                var body = new com.google.gson.JsonObject();
                                body.addProperty("id", a.get("id").getAsString());
                                var r = Api.call("POST", "/account/select", body);
                                client.execute(() -> System.out.println("[Noxel] проверка смены: " + AccountScreen.switchUser(r)));
                            }
                        }
                    } catch (Exception ex) {
                        System.out.println("[Noxel] проверка смены не удалась: " + ex.getMessage());
                    }
                });
            }
            if (System.getProperty("noxel.selftest") != null && screen instanceof TitleScreen && !selfOpened) {
                selfOpened = true;                           // проверка из лаунчера: сама открывает экран друзей
                client.execute(() -> client.setScreenAndShow(new FriendsScreen(screen)));
            }
        });
    }
}
