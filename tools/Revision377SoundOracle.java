// Executes untouched Java 377 Wave.unpack, including method481 lead-in trimming.
import jagex2.io.Jagfile;
import jagex2.io.Packet;
import jagex2.sound.Tone;
import jagex2.sound.Wave;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

public final class Revision377SoundOracle {
    public static void main(String[] args) throws Exception {
        Jagfile jag = new Jagfile(Files.readAllBytes(Path.of(args[0])));
        Packet packet = new Packet(jag.read("sounds.dat", null));
        Wave.unpack(packet, 36135);
        if (packet.pos != packet.data.length) throw new AssertionError("sounds.dat tail");
        List<Object> rows = new ArrayList<>();
        for (int id = 0; id < Wave.field1471.length; id++) {
            Wave wave = Wave.field1471[id];
            if (wave == null) { rows.add(null); continue; }
            List<Object> tones = new ArrayList<>();
            for (Tone tone : wave.field1475) {
                tones.add(tone == null ? null : Arrays.asList(tone.field661, tone.field662));
            }
            rows.add(Arrays.asList(Wave.field1472[id], wave.field1476, wave.field1477, tones));
        }
        StringBuilder output = new StringBuilder();
        Revision377DecoderOracle.json(Arrays.asList(packet.pos, rows), output);
        Files.writeString(Path.of(args[1]), output.toString());
    }
}
