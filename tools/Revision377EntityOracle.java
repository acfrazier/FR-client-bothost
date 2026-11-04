import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import jagex2.io.Packet;

/** Control-flow adapter for Client.java:8087-8304,9015-9577. All primitive
 * transforms and bit reads execute the unchanged original Java Packet class.
 * It does not instantiate Client, resolve cache definitions or apply updates. */
public final class Revision377EntityOracle {
    private static List<Object> a(Object... values) { return Arrays.asList(values); }
    private static String hex(byte[] bytes) {
        StringBuilder out=new StringBuilder();
        for(byte value:bytes) out.append(String.format("%02x",value&255));
        return out.toString();
    }
    public static String json(Object value) {
        if(value==null) return "null";
        if(value instanceof String) {
            StringBuilder out=new StringBuilder("\"");
            for(char c:((String)value).toCharArray()) {
                if(c=='\\'||c=='\"') out.append('\\').append(c);
                else if(c<32||c>126) out.append(String.format("\\u%04x",(int)c));
                else out.append(c);
            }
            return out.append('"').toString();
        }
        if(value instanceof List) {
            StringBuilder out=new StringBuilder("["); boolean first=true;
            for(Object entry:(List<?>)value) {if(!first)out.append(',');first=false;out.append(json(entry));}
            return out.append(']').toString();
        }
        return value.toString();
    }
    private static final class R {
        final Packet p;
        final List<Object> reads=new ArrayList<>(),values=new ArrayList<>();
        R(byte[] bytes){p=new Packet(bytes);}
        int g(String name) throws Exception {
            Method method=Packet.class.getMethod(name);
            int value=((Number)method.invoke(p)).intValue();
            reads.add(name);values.add(value);return value;
        }
        String text() {String value=p.gjstr();reads.add("gjstr");values.add(value);return value;}
        void bits(){p.accessBits();reads.add("accessBits");values.add(null);}
        int bit(int width){int value=p.gBit(width);reads.add("gBit:"+width);values.add(value);return value;}
        void bytes(){p.accessBytes();reads.add("accessBytes");values.add(null);}
        byte[] data(String method,int count) {
            byte[] bytes=new byte[count];
            if(method.equals("gdata_alt1"))p.gdata_alt1(bytes,count,0);
            else p.gdata_alt2(bytes,count,0);
            reads.add(method+":"+count);values.add(hex(bytes));return bytes;
        }
    }
    private static Object optional(int value){return value==65535?null:value;}
    private static Object movement(R r, boolean local) {
        if(r.bit(1)==0)return a("Idle");
        int op=r.bit(2);
        if(op==0)return a("Extended");
        if(op==1)return a("Walk",r.bit(3),r.bit(1)!=0);
        if(op==2)return a("Run",r.bit(3),r.bit(3),r.bit(1)!=0);
        if(!local)return a("Remove");
        boolean jump=r.bit(1)!=0;int plane=r.bit(2),z=r.bit(7),x=r.bit(7);
        return a("Teleport",jump,plane,x,z,r.bit(1)!=0);
    }
    private static boolean extended(Object move) {
        List<?> v=(List<?>)move;
        return v.get(0).equals("Extended")||((v.get(0).equals("Walk")||v.get(0).equals("Run")||v.get(0).equals("Teleport"))&&(Boolean)v.get(v.size()-1));
    }
    // ClientPlayer.java:332-395. No model assembly or cache lookup is performed.
    private static Object appearance(byte[] bytes) {
        Packet p=new Packet(bytes);
        int gender=p.g1(),head=p.g1b(),skull=p.g1b();
        List<Object> equipment=new ArrayList<>(),colours=new ArrayList<>(),sequences=new ArrayList<>();
        Object npc=null;
        for(int slot=0;slot<12;slot++) {
            int high=p.g1(),value=high==0?0:(high<<8)+p.g1();equipment.add(value);
            if(slot==0&&value==65535){npc=p.g2();while(equipment.size()<12)equipment.add(0);break;}
        }
        for(int i=0;i<5;i++)colours.add(p.g1());
        for(int i=0;i<7;i++)sequences.add(optional(p.g2()));
        String name=Long.toString(p.g8());int combat=p.g1(),total=p.g2();
        if(p.pos!=bytes.length)throw new IllegalArgumentException("appearance under-consumption");
        return a(hex(bytes),gender,head,skull,equipment,npc,colours,sequences,name,combat,total);
    }
    private static Object playerBlock(R r,Object target) throws Exception {
        int mask=r.g("g1");if((mask&32)!=0)mask|=r.g("g1")<<8;
        Object anim=null,say=null,exact=null,face=null,coord=null,spot=null,appearance=null,damage=null,chat=null,damage2=null;
        if((mask&8)!=0)anim=a(optional(r.g("g2")),r.g("g1_alt3"));
        if((mask&16)!=0)say=r.text();
        if((mask&256)!=0)exact=a(r.g("g1_alt1"),r.g("g1_alt2"),r.g("g1_alt3"),r.g("g1"),r.g("g2"),r.g("g2_alt2"),r.g("g1"));
        if((mask&1)!=0)face=optional(r.g("g2_alt2"));
        if((mask&2)!=0)coord=a(r.g("g2"),r.g("g2"));
        if((mask&512)!=0){Object id=optional(r.g("g2_alt2"));int packed=r.g("g4_alt2");spot=a(id,packed>>16,packed&65535);}
        if((mask&4)!=0)appearance=appearance(r.data("gdata_alt1",r.g("g1")));
        if((mask&1024)!=0)damage=a(r.g("g1_alt1"),r.g("g1_alt3"),r.g("g1_alt2"),r.g("g1"));
        if((mask&64)!=0){int colourEffect=r.g("g2"),staff=r.g("g1_alt2"),length=r.g("g1_alt1");chat=a(colourEffect,staff,hex(r.data("gdata_alt2",length)));}
        if((mask&128)!=0)damage2=a(r.g("g1_alt3"),r.g("g1_alt2"),r.g("g1_alt3"),r.g("g1"));
        return a(target,mask,anim,say,exact,face,coord,spot,appearance,damage,chat,damage2);
    }
    private static Object npcBlock(R r,Object target) throws Exception {
        int mask=r.g("g1");Object type=null,face=null,damage=null,spot=null,say=null,coord=null,anim=null,damage2=null;
        if((mask&1)!=0)type=r.g("g2_alt2");
        if((mask&64)!=0)face=optional(r.g("g2_alt1"));
        if((mask&128)!=0)damage=a(r.g("g1_alt1"),r.g("g1_alt1"),r.g("g1"),r.g("g1_alt3"));
        if((mask&4)!=0){Object id=optional(r.g("g2"));int packed=r.g("g4_alt2");spot=a(id,packed>>16,packed&65535);}
        if((mask&32)!=0)say=r.text();
        if((mask&8)!=0)coord=a(r.g("g2_alt3"),r.g("g2_alt1"));
        if((mask&2)!=0)anim=a(optional(r.g("g2")),r.g("g1_alt3"));
        if((mask&16)!=0)damage2=a(r.g("g1_alt3"),r.g("g1_alt3"),r.g("g1"),r.g("g1_alt2"));
        return a(target,mask,type,face,damage,spot,say,coord,anim,damage2);
    }
    public static String inspect(String mode,byte[] bytes) throws Exception {
        R r=new R(bytes);Object fields;
        if(mode.equals("rebuild_region")) {
            int x=r.g("g2_alt2");r.bits();List<Object> templates=new ArrayList<>();
            for(int i=0;i<676;i++)templates.add(r.bit(1)==0?-1:r.bit(26));
            r.bytes();int z=r.g("g2_alt2");fields=a("RebuildInstance",x,z,templates);
        } else {
            boolean player=mode.equals("player_info");
            List<Object> old=new ArrayList<>(),added=new ArrayList<>(),targets=new ArrayList<>(),updates=new ArrayList<>();
            r.bits();Object local=player?movement(r,true):null;
            if(player&&extended(local))targets.add(a("Local"));
            int count=r.bit(8);
            for(int i=0;i<count;i++){Object move=movement(r,false);old.add(move);if(extended(move))targets.add(a("Old",i));}
            int width=player?11:14,sentinel=player?2047:16383;
            while(r.p.bitPos+(player?10:21)<bytes.length*8) {
                int slot=r.bit(width);if(slot==sentinel)break;
                if(player){int x=r.bit(5);boolean extend=r.bit(1)!=0,jump=r.bit(1)!=0;int z=r.bit(5);added.add(a(slot,x>15?x-32:x,z>15?z-32:z,jump,extend));if(extend)targets.add(a("New",slot));}
                else {boolean extend=r.bit(1)!=0;int z=r.bit(5),x=r.bit(5);boolean jump=r.bit(1)!=0;int type=r.bit(13);added.add(a(slot,type,x>15?x-32:x,z>15?z-32:z,jump,extend));if(extend)targets.add(a("New",slot));}
            }
            r.bytes();
            for(Object target:targets)updates.add(player?playerBlock(r,target):npcBlock(r,target));
            fields=player?a("PlayerInfo",local,count,old,added,updates):a("NpcInfo",count,old,added,updates);
        }
        if(r.p.pos!=bytes.length)throw new IllegalArgumentException("entity/rebuild under-consumption: "+r.p.pos+"/"+bytes.length);
        return "{\"values\":"+json(r.values)+",\"reads\":"+json(r.reads)+",\"operation_fields\":"+json(fields)+",\"consumed\":"+r.p.pos+"}";
    }
}
