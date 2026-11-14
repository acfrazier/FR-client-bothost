// Runs the original Client-Java 377 decoders. No target source is rewritten.
// The Python driver records source/cache hashes and reduces this full field dump.
import jagex2.config.*;
import jagex2.io.*;
import java.lang.reflect.*;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public final class Revision377DecoderOracle {
    static Map<String, Object> fields(Object obj) throws Exception {
        Map<String, Object> result = new TreeMap<>();
        for (Field field : obj.getClass().getFields()) {
            if (Modifier.isStatic(field.getModifiers())) continue;
            Class<?> type = field.getType();
            while (type.isArray()) type = type.getComponentType();
            if (type.isPrimitive() || type == String.class) result.put(field.getName(), field.get(obj));
        }
        return result;
    }
    static List<Object> indexed(Jagfile jag, String name) throws Exception {
        Packet dat = new Packet(jag.read(name + ".dat", null));
        Packet idx = new Packet(jag.read(name + ".idx", null));
        int count = idx.g2();
        if (dat.g2() != count) throw new AssertionError(name + " header");
        List<Object> rows = new ArrayList<>();
        for (int id = 0; id < count; id++) {
            int end = dat.pos + idx.g2();
            Object obj;
            if (name.equals("loc")) {
                LocType loc = new LocType(); loc.field1627 = id; loc.method567(); loc.method568(dat); obj = loc;
            } else if (name.equals("npc")) {
                NpcType npc = new NpcType(); npc.field1431 = id; npc.method470(dat); obj = npc;
            } else {
                ObjType item = new ObjType(); item.field845 = id; item.method232(); item.method227(dat); obj = item;
            }
            if (dat.pos != end) throw new AssertionError(name + " record " + id + " end " + dat.pos + " != " + end);
            rows.add(fields(obj));
        }
        if (dat.pos != dat.data.length || idx.pos != idx.data.length) throw new AssertionError(name + " tail");
        return rows;
    }
    static List<Object> array(Object[] values) throws Exception {
        List<Object> rows = new ArrayList<>();
        for (Object value : values) rows.add(fields(value));
        return rows;
    }
    static void json(Object value, StringBuilder out) {
        if (value == null) { out.append("null"); return; }
        if (value instanceof Number || value instanceof Boolean) { out.append(value); return; }
        if (value instanceof String) {
            out.append('"');
            for (char c : ((String)value).toCharArray()) {
                switch (c) {
                    case '"': out.append("\\\""); break;
                    case '\\': out.append("\\\\"); break;
                    case '\n': out.append("\\n"); break;
                    case '\r': out.append("\\r"); break;
                    case '\t': out.append("\\t"); break;
                    case '\b': out.append("\\b"); break;
                    case '\f': out.append("\\f"); break;
                    default: if (c < 32) out.append(String.format("\\u%04x", (int)c)); else out.append(c);
                }
            }
            out.append('"'); return;
        }
        if (value instanceof Map) {
            out.append('{'); boolean first = true;
            for (Object entryObject : ((Map<?, ?>)value).entrySet()) {
                Map.Entry<?, ?> entry = (Map.Entry<?, ?>)entryObject;
                if (!first) out.append(','); first = false;
                json(entry.getKey(), out); out.append(':'); json(entry.getValue(), out);
            }
            out.append('}'); return;
        }
        out.append('['); boolean first = true;
        if (value.getClass().isArray()) {
            for (int i = 0; i < Array.getLength(value); i++) {
                if (!first) out.append(','); first = false; json(Array.get(value, i), out);
            }
        } else {
            for (Object item : (Iterable<?>)value) {
                if (!first) out.append(','); first = false; json(item, out);
            }
        }
        out.append(']');
    }
    public static void main(String[] args) throws Exception {
        Path cache = Paths.get(args[0]);
        Jagfile config = new Jagfile(Files.readAllBytes(cache.resolve("config")));
        Map<String, Object> tables = new TreeMap<>();
        // Same startup order as Client.java:2046-2054.
        SeqType.unpack(config);
        tables.put("seq", array(SeqType.field775));
        tables.put("loc", indexed(config, "loc"));
        FloType.unpack(config);
        tables.put("flo", array(FloType.field796));
        tables.put("obj", indexed(config, "obj"));
        tables.put("npc", indexed(config, "npc"));
        IdkType.unpack(config);
        tables.put("idk", array(IdkType.field1699));
        SpotAnimType.unpack(config);
        tables.put("spotanim", array(SpotAnimType.field1297));
        VarpType.unpack(config);
        VarbitType.unpack(config);
        tables.put("varp", array(VarpType.field1507));
        tables.put("varbit", array(VarbitType.field1760));
        tables.put("tracked_varps", Arrays.copyOf(VarpType.field1509, VarpType.field1508));
        // Exercise Java's actual get path independently of raw-record decode.
        List<Integer> objectSamples = new ArrayList<>();
        objectSamples.add(0);
        List<?> objects = (List<?>) tables.get("obj");
        for (String criterion : new String[]{"certificate", "member", "member_certificate"}) {
            for (int id = 0; id < objects.size(); id++) {
                Map<?, ?> row = (Map<?, ?>) objects.get(id);
                boolean certificate = ((Number)row.get("field825")).intValue() != -1;
                boolean member = (Boolean)row.get("field859");
                if ((criterion.equals("certificate") && certificate)
                    || (criterion.equals("member") && member)
                    || (criterion.equals("member_certificate") && certificate && member)) {
                    if (!objectSamples.contains(id)) objectSamples.add(id);
                    break;
                }
            }
        }
        for (boolean members : new boolean[]{true, false}) {
            ObjType.unpack(config);
            ObjType.membersWorld = members;
            Map<String, Object> sample = new TreeMap<>();
            for (int id : objectSamples) sample.put(Integer.toString(id), fields(ObjType.get(id)));
            tables.put(members ? "obj_members" : "obj_nonmembers", sample);
        }
        Jagfile interfaces = new Jagfile(Files.readAllBytes(cache.resolve("interface")));
        // Component.unpack initializes this before decode (Component.java:558).
        Component.imageCache = new jagex2.datastruct.LruCache(50000);
        Packet data = new Packet(interfaces.read("data", null));
        int total = data.g2(), layer = -1;
        List<Object> components = new ArrayList<>();
        for (int i = 0; i < total; i++) components.add(null);
        List<Object> raw = new ArrayList<>();
        while (data.pos < data.data.length) {
            int id = data.g2();
            if (id == 65535) { layer = data.g2(); id = data.g2(); }
            int start = data.pos;
            Component com = Component.decode(layer, data, id);
            if (components.set(id, fields(com)) != null) throw new AssertionError("duplicate component " + id);
            Map<String, Object> record = new TreeMap<>();
            record.put("id", id); record.put("layer", layer); record.put("start", start); record.put("end", data.pos);
            raw.add(record);
        }
        tables.put("interface", components);
        tables.put("interface_records", raw);
        tables.put("interface_layers", new int[]{Component.field728, Component.field737, Component.field759});
        StringBuilder out = new StringBuilder(); json(tables, out);
        Files.write(Paths.get(args[1]), out.toString().getBytes(StandardCharsets.UTF_8));
        System.out.println("PASS: original Java decoded all nine config tables and " + raw.size() + " interface records in " + total + " slots");
    }
}
