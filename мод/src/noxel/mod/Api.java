package noxel.mod;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;

/** Связь с лаунчером Noxel: локальный адрес и ключ лаунчер передаёт игре при запуске. Токен аккаунта мод не видит. */
public final class Api {
    static final String BASE = System.getProperty("noxel.api");
    static final String KEY = System.getProperty("noxel.key");
    private static final HttpClient HTTP = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(3)).build();

    private Api() {}

    public static boolean available() {
        return BASE != null && KEY != null;
    }

    public static JsonObject call(String method, String path, JsonObject body) throws Exception {
        HttpRequest.Builder b = HttpRequest.newBuilder(URI.create(BASE + path)).timeout(Duration.ofSeconds(90)).header("X-Noxel-Key", KEY).header("Content-Type", "application/json");
        b = method.equals("GET") ? b.GET() : b.POST(HttpRequest.BodyPublishers.ofString(body == null ? "{}" : body.toString()));
        HttpResponse<String> r = HTTP.send(b.build(), HttpResponse.BodyHandlers.ofString());
        JsonObject o = JsonParser.parseString(r.body()).getAsJsonObject();
        if (r.statusCode() != 200) throw new Exception(o.has("error") ? o.get("error").getAsString() : "ошибка " + r.statusCode());
        return o;
    }
}
