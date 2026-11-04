import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.lang.reflect.Method;
import jagex2.io.Packet;

/** Calls untouched Java Packet readers. Control-flow expansion is supplied by
 * the generator from Client.java; this helper does not translate transforms. */
public final class Revision377PacketOracle {
    private static String quote(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c == '\\' || c == '"') out.append('\\').append(c);
            else if (c < 32 || c > 126) out.append(String.format("\\u%04x", (int) c));
            else out.append(c);
        }
        return out.append('"').toString();
    }
    public static void main(String[] args) throws Exception {
        BufferedReader input = new BufferedReader(new InputStreamReader(System.in));
        String line;
        while ((line = input.readLine()) != null) {
            String[] parts = line.split("\\t", -1);
            byte[] bytes = new byte[parts[0].length() / 2];
            for (int i = 0; i < bytes.length; i++) {
                bytes[i] = (byte) Integer.parseInt(parts[0].substring(i * 2, i * 2 + 2), 16);
            }
            Packet packet = new Packet(bytes);
            StringBuilder out = new StringBuilder("{\"values\":[");
            boolean first = true;
            if (!parts[1].isEmpty()) for (String name : parts[1].split(",")) {
                if (!first) out.append(',');
                first = false;
                if (name.equals("rest")) {
                    StringBuilder hex = new StringBuilder();
                    while (packet.pos < bytes.length) hex.append(String.format("%02x", packet.g1()));
                    out.append(quote(hex.toString()));
                } else {
                    Method method = Packet.class.getMethod(name);
                    Object value = method.invoke(packet);
                    if (value instanceof String) out.append(quote((String) value));
                    else if (value instanceof Long) out.append(quote(value.toString()));
                    else out.append(value.toString());
                }
            }
            out.append("],\"consumed\":").append(packet.pos).append('}');
            System.out.println(out);
        }
    }
}
