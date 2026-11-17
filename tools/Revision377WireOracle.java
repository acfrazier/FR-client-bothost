import jagex2.io.Packet;
import jagex2.io.Isaac;
import java.math.BigInteger;
import java.util.HexFormat;
import java.util.Arrays;

public final class Revision377WireOracle {
    static String hex(Packet p) {
        return HexFormat.of().formatHex(Arrays.copyOf(p.data, p.pos));
    }
    public static void main(String[] args) {
        int[] seed = {1, 2, 0x12345678, -1};
        System.out.print("{\"seed\":[1,2,305419896,-1],\"isaac\":[");
        for (int direction = 0; direction < 2; direction++) {
            int[] words = seed.clone();
            for (int i = 0; i < 4; i++) words[i] += direction * 50;
            Isaac cipher = new Isaac(words);
            if (direction > 0) System.out.print(",");
            System.out.print("[");
            for (int i = 0; i < 1025; i++) {
                if (i > 0) System.out.print(",");
                System.out.print(cipher.nextInt());
            }
            System.out.print("]");
        }
        System.out.print("],\"rsa\":[");
        String[] values = {"00", "01", "7f", "0080", "00ff", "0100", "0101"};
        for (int i = 0; i < values.length; i++) {
            Packet p = Packet.alloc(1);
            byte[] raw = HexFormat.of().parseHex(values[i]);
            p.pdata(raw, raw.length, 0);
            p.rsaenc(BigInteger.valueOf(257), BigInteger.ONE);
            if (i > 0) System.out.print(",");
            System.out.print("{\"plaintext\":\"" + values[i] + "\",\"modulus\":257,\"exponent\":1,\"frame\":\"" + hex(p) + "\"}");
        }
        Packet block = Packet.alloc(1);
        block.p1(10);
        for (int word : seed) block.p4(word);
        block.p4(0x76543210);
        block.pjstr("alice");
        block.pjstr("test");
        System.out.print("],\"plaintext\":\"" + hex(block) + "\",\"wrappers\":[");
        block.rsaenc(BigInteger.ONE.shiftLeft(512).subtract(BigInteger.ONE), BigInteger.ONE);
        for (int reconnect = 0; reconnect < 2; reconnect++) {
            Packet login = Packet.alloc(1);
            login.p1(reconnect == 1 ? 18 : 16);
            login.p1(block.pos + 36 + 1 + 1 + 2);
            login.p1(255);
            login.p2(377);
            login.p1(reconnect);
            for (int i = 0; i < 9; i++) login.p4(0x10203040 + i);
            login.pdata(block.data, block.pos, 0);
            if (reconnect > 0) System.out.print(",");
            System.out.print("\"" + hex(login) + "\"");
        }
        System.out.println("]}");
    }
}
