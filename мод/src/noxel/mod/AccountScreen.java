package noxel.mod;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.Field;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.client.Minecraft;
import net.minecraft.client.User;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

/** Выбор аккаунта прямо в игре: лаунчер запоминает его как активный, а имя в игре меняется для следующего входа на сервер или в мир. */
public final class AccountScreen extends Screen {
    private final Screen parent;
    private JsonArray accounts;
    private String status = "Загружаю…";

    AccountScreen(Screen parent) {
        super(Component.literal("Аккаунт Noxel"));
        this.parent = parent;
    }

    private static Minecraft mc() {
        return Minecraft.getInstance();
    }

    @Override
    protected void init() {
        int cx = width / 2;
        addRenderableWidget(Button.builder(Component.literal("Назад"), b -> onClose()).bounds(cx + 62, 34, 88, 20).build());
        if (accounts != null) {
            for (int i = 0; i < Math.min(accounts.size(), 8); i++) {
                JsonObject a = accounts.get(i).getAsJsonObject();
                if (a.get("selected").getAsBoolean()) continue;
                String id = a.get("id").getAsString();
                addRenderableWidget(Button.builder(Component.literal("Выбрать"), b -> choose(id)).bounds(cx + 82, 64 + i * 24, 68, 20).build());
            }
        }
        if (accounts == null) load();
    }

    private void load() {
        CompletableFuture.runAsync(() -> {
            try {
                JsonObject o = Api.call("GET", "/accounts", null);
                mc().execute(() -> {
                    accounts = o.getAsJsonArray("accounts");
                    status = "";
                    rebuildWidgets();
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    private void choose(String id) {
        JsonObject b = new JsonObject();
        b.addProperty("id", id);
        status = "Меняю аккаунт…";
        CompletableFuture.runAsync(() -> {
            try {
                JsonObject r = Api.call("POST", "/account/select", b);
                String res = switchUser(r);
                mc().execute(() -> {
                    status = res;
                    accounts = null;
                    rebuildWidgets();
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    /** Подменяет пользователя игры; новое имя будет использовано при следующем входе на сервер или в мир. Возвращает пояснение. */
    static String switchUser(JsonObject r) {
        String name = r.get("name").getAsString();
        try {
            String raw = r.get("uuid").getAsString().replace("-", "");
            UUID uuid = UUID.fromString(raw.replaceFirst("(\\p{XDigit}{8})(\\p{XDigit}{4})(\\p{XDigit}{4})(\\p{XDigit}{4})(\\p{XDigit}+)", "$1-$2-$3-$4-$5"));
            String xuid = r.has("xuid") && !r.get("xuid").getAsString().isEmpty() ? r.get("xuid").getAsString() : "0";
            User u = new User(name, uuid, r.get("token").getAsString(), Optional.of(xuid), Optional.of("noxel"));
            Field f = Minecraft.class.getDeclaredField("user");
            f.setAccessible(true);
            f.set(mc(), u);
            System.out.println("[Noxel] аккаунт сменён на " + mc().getUser().getName());
            return "Теперь ты играешь как " + name + ". Новое имя применится при следующем входе на сервер или в мир";
        } catch (Throwable t) {
            System.out.println("[Noxel] не удалось сменить имя в игре: " + t);
            return "Аккаунт " + name + " выбран в лаунчере. Перезапусти игру, чтобы войти под ним";
        }
    }

    @Override
    public void extractRenderState(GuiGraphicsExtractor g, int mx, int my, float delta) {
        super.extractRenderState(g, mx, my, delta);
        int cx = width / 2;
        g.text(font, "Аккаунт Noxel  ·  сейчас: " + mc().getUser().getName(), cx - 150, 14, 0xFFFFFFFF);
        if (accounts != null) {
            for (int i = 0; i < Math.min(accounts.size(), 8); i++) {
                JsonObject a = accounts.get(i).getAsJsonObject();
                boolean on = a.get("selected").getAsBoolean();
                String kind = switch (a.get("kind").getAsString()) { case "microsoft" -> "лицензия"; case "noxel" -> "Noxel"; default -> "локальный"; };
                g.text(font, (on ? "● " : "○ ") + a.get("name").getAsString() + "  " + kind, cx - 150, 70 + i * 24, on ? 0xFFFFFFFF : 0xFFB0B0B0);
            }
        }
        if (!status.isEmpty()) g.text(font, status, cx - 150, height - 40, 0xFFCCCCCC);
    }

    @Override
    public void onClose() {
        mc().setScreenAndShow(parent);
    }
}
