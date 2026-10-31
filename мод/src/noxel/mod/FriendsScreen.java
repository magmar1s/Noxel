package noxel.mod;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.EditBox;
import net.minecraft.client.gui.screens.ConnectScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.resolver.ServerAddress;
import net.minecraft.network.chat.Component;

/** Экран друзей: список, заявки, мой мир для друзей, заход в мир друга, мир в облако со ссылкой. */
public final class FriendsScreen extends Screen {
    private final Screen parent;
    private JsonObject data;
    private String status = "Загружаю…";
    private long lastLoad;
    private EditBox name;
    private boolean logged, rendered;

    FriendsScreen(Screen parent) {
        super(Component.literal("Друзья Noxel"));
        this.parent = parent;
    }

    private static Minecraft mc() {
        return Minecraft.getInstance();
    }

    private boolean hosting() {
        return data != null && data.has("room") && data.getAsJsonObject("room").has("hosting") && !data.getAsJsonObject("room").get("hosting").isJsonNull();
    }

    /** Строки списка: сначала заявки, затем друзья (не больше восьми). */
    private List<String[]> rows() {
        List<String[]> out = new ArrayList<>();
        if (data == null) return out;
        for (var e : data.getAsJsonArray("incoming")) out.add(new String[]{"req", e.getAsString(), "", ""});
        for (var e : data.getAsJsonArray("friends")) {
            JsonObject f = e.getAsJsonObject();
            String game = f.get("game").isJsonNull() ? "" : f.get("game").getAsString();
            String st = f.get("online").getAsBoolean() ? "в сети" + (game.isEmpty() ? "" : ", играет " + game) : "не в сети";
            out.add(new String[]{f.get("room").getAsBoolean() ? "room" : "friend", f.get("name").getAsString(), st, f.get("online").getAsBoolean() ? "1" : "0"});
        }
        return out.size() > 8 ? out.subList(0, 8) : out;
    }

    @Override
    protected void init() {
        int cx = width / 2;
        String keep = name == null ? "" : name.getValue();
        name = addRenderableWidget(new EditBox(font, cx - 150, 34, 130, 20, Component.literal("Ник")));
        name.setMaxLength(16);
        name.setValue(keep);
        addRenderableWidget(Button.builder(Component.literal("Добавить"), b -> act("add", name.getValue().trim())).bounds(cx - 14, 34, 70, 20).build());
        addRenderableWidget(Button.builder(Component.literal("Назад"), b -> onClose()).bounds(cx + 62, 34, 88, 20).build());
        boolean host = hosting();
        addRenderableWidget(Button.builder(Component.literal(host ? "Закрыть мой мир" : "Открыть мой мир"), b -> toggleRoom(host)).bounds(cx - 150, height - 30, 148, 20).build());
        addRenderableWidget(Button.builder(Component.literal("Мир в облако + ссылка"), b -> saveCloud()).bounds(cx + 2, height - 30, 148, 20).build());
        int i = 0;
        for (String[] r : rows()) {
            int y = 64 + i * 24;
            String n = r[1];
            if (r[0].equals("req")) addRenderableWidget(Button.builder(Component.literal("Принять"), b -> act("accept", n)).bounds(cx + 82, y, 68, 20).build());
            else if (r[0].equals("room")) addRenderableWidget(Button.builder(Component.literal("Зайти"), b -> join(n)).bounds(cx + 82, y, 68, 20).build());
            i++;
        }
    }

    @Override
    public void tick() {
        if (System.currentTimeMillis() - lastLoad > 8000) load();
    }

    private void load() {
        lastLoad = System.currentTimeMillis();
        CompletableFuture.runAsync(() -> {
            try {
                JsonObject o = Api.call("GET", "/friends", null);
                mc().execute(() -> {
                    data = o;
                    if (status.equals("Загружаю…")) status = "";
                    if (!logged) {
                        logged = true;
                        System.out.println("[Noxel] друзей загружено: " + o.getAsJsonArray("friends").size());
                    }
                    rebuildWidgets();
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    private void act(String action, String who) {
        if (who.isEmpty()) return;
        JsonObject b = new JsonObject();
        b.addProperty("action", action);
        b.addProperty("name", who);
        status = "…";
        CompletableFuture.runAsync(() -> {
            try {
                JsonObject r = Api.call("POST", "/friend", b);
                String st = r.get("state").getAsString();
                mc().execute(() -> {
                    status = st.equals("accepted") ? r.get("name").getAsString() + " теперь в друзьях" : "Заявка отправлена: " + r.get("name").getAsString();
                    if (name != null) name.setValue("");
                    lastLoad = 0;
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    private void toggleRoom(boolean close) {
        status = close ? "Закрываю мир…" : "Открываю мир для друзей…";
        CompletableFuture.runAsync(() -> {
            try {
                Api.call("POST", close ? "/room/close" : "/room/open", null);
                mc().execute(() -> {
                    status = close ? "Мир закрыт для друзей" : "Мир открыт: друзья видят его в списке и могут зайти";
                    lastLoad = 0;
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    private void saveCloud() {
        status = "Сохраняю мир в облако (лучше сначала сохранить игру)…";
        CompletableFuture.runAsync(() -> {
            try {
                JsonObject r = Api.call("POST", "/cloud/save", null);
                String url = r.get("url").getAsString();
                mc().execute(() -> {
                    mc().keyboardHandler.setClipboard(url);
                    status = "Ссылка скопирована: " + url;
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    private void join(String host) {
        JsonObject b = new JsonObject();
        b.addProperty("host", host);
        status = "Подключаюсь к миру " + host + "…";
        CompletableFuture.runAsync(() -> {
            try {
                int port = Api.call("POST", "/room/join", b).get("port").getAsInt();
                mc().execute(() -> {
                    Minecraft m = mc();
                    String addr = "127.0.0.1:" + port;
                    if (m.level != null) m.disconnect(new TitleScreen(), false);   // выходим из текущего мира
                    ConnectScreen.startConnecting(new TitleScreen(), m, ServerAddress.parseString(addr), new ServerData("Мир друга " + host, addr, ServerData.Type.OTHER), false, null);
                });
            } catch (Exception e) {
                mc().execute(() -> status = e.getMessage());
            }
        });
    }

    @Override
    public void extractRenderState(GuiGraphicsExtractor g, int mx, int my, float delta) {
        super.extractRenderState(g, mx, my, delta);
        int cx = width / 2;
        g.text(font, "Друзья Noxel" + (data != null ? "  ·  ты: " + data.get("me").getAsString() : ""), cx - 150, 14, 0xFFFFFFFF);
        int i = 0;
        for (String[] r : rows()) {
            int y = 70 + i * 24;
            String line = r[0].equals("req") ? "Заявка в друзья: " + r[1] : (r[3].equals("1") ? "● " : "○ ") + r[1] + "  " + r[2] + (r[0].equals("room") ? "  · мир открыт" : "");
            g.text(font, line, cx - 150, y, r[0].equals("req") || r[3].equals("1") ? 0xFFFFFFFF : 0xFF9A9A9A);
            i++;
        }
        if (data != null && rows().isEmpty()) g.text(font, "Пока никого нет: введи ник друга из Noxel и нажми «Добавить»", cx - 150, 70, 0xFF9A9A9A);
        if (!status.isEmpty()) g.text(font, status, cx - 150, height - 46, 0xFFCCCCCC);
        if (!rendered) {
            rendered = true;
            System.out.println("[Noxel] экран друзей отрисован");
        }
    }

    @Override
    public void onClose() {
        mc().setScreenAndShow(parent);
    }
}
