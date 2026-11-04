// Execute the unchanged rsbuf producers behind the two byte-forwarding codecs.
// This builds synthetic entities only; it never starts an engine or a socket.
export async function entityPayloads(module) {
    const { Player, Chat, ExactMove } = await module('player');
    const { Npc } = await module('npc');
    const { PlayerRenderer, NpcRenderer } = await module('renderer');
    const { PlayerInfoEncoder, NpcInfoEncoder } = await module('info');
    const { PlayerInfoProt: P, NpcInfoProt: N } = await module('prot');
    const { CoordGrid } = await module('coord');
    const { ZoneMap } = await module('grid');
    const { Packet } = await module('packet');
    const appearance = new Packet(100);
    appearance.p1(1); appearance.p1(2); appearance.p1(-1);
    appearance.p2(261); appearance.p2(522);
    for (let i=2;i<12;i++) appearance.p1(0);
    for (const colour of [1,2,3,4,5]) appearance.p1(colour);
    for (const sequence of [100,101,102,103,104,105,-1]) appearance.p2(sequence);
    appearance.p4(0x01020304); appearance.p4(0x05060708);
    appearance.p1(77); appearance.p2(1234);
    const appearanceBytes = appearance.data.slice(0,appearance.pos);
    const player = new Player(1);
    player.active=true; player.tele=true; player.jump=true;
    player.coord=CoordGrid.from(3203,2,3205); player.origin=CoordGrid.from(3200,2,3200);
    const players=Array(2048).fill(null); players[1]=player;
    const playerMap=new ZoneMap();
    const playerRenderer=new PlayerRenderer();
    for (let i=0;i<5;i++) {
        const other=new Player(10+i);
        other.active=i!==4; other.coord=CoordGrid.from(3204,2,3206);
        players[other.pid]=other; player.build.players.insert(other.pid);
        if(i===1) {
            other.masks=Object.values(P).filter(value=>typeof value==='number'&&value!==P.BIG).reduce((mask,value)=>mask|value,0);
            other.appearance=appearanceBytes; other.lastAppearance=17;
            other.animId=321; other.animDelay=7; other.say='~all player masks';
            other.faceEntity=32768+23; other.faceX=6411; other.faceZ=6413;
            other.graphicId=456; other.graphicHeight=23; other.graphicDelay=234;
            other.damageTaken=19; other.damageType=2; other.currentHitpoints=67; other.baseHitpoints=99;
            other.damageTaken2=37; other.damageType2=3;
            other.chat=new Chat(Uint8Array.from([0x12,0xab,0x7f]),4,5,2);
            other.exactMove=new ExactMove(3163,3172,3181,3190,123,234,3);
        }
        if(i===2) other.walkDir=5;
        if(i===3) { other.walkDir=1; other.runDir=6; }
        playerRenderer.computeInfo(other);
    }
    const added=new Player(23); added.active=true; added.jump=true;
    added.coord=CoordGrid.from(3201,2,3208); added.appearance=appearanceBytes;
    added.masks=P.APPEARANCE; added.lastAppearance=18;
    players[23]=added; playerMap.zone(3201,2,3208).addPlayer(23); playerRenderer.computeInfo(added);
    const playerData=new PlayerInfoEncoder().encode(0,playerRenderer,players,playerMap,new Map(),player,0,0,false);
    const npcPlayer=new Player(1); npcPlayer.active=true; npcPlayer.coord=player.coord; npcPlayer.origin=player.origin;
    const npcs=Array(16384).fill(null), npcMap=new ZoneMap(), npcRenderer=new NpcRenderer();
    for(let i=0;i<5;i++) {
        const npc=new Npc(110+i,345);
        npc.active=i!==4; npc.coord=CoordGrid.from(3204,2,3206);
        npcs[npc.nid]=npc; npcPlayer.build.npcs.insert(npc.nid);
        if(i===1) {
            npc.masks=255; npc.animId=321; npc.animDelay=7; npc.say='all npc masks';
            npc.faceEntity=32768+23; npc.faceX=6411; npc.faceZ=6413;
            npc.graphicId=456; npc.graphicHeight=23; npc.graphicDelay=234;
            npc.damageTaken=19; npc.damageType=2; npc.currentHitpoints=67; npc.baseHitpoints=99;
            npc.damageTaken2=37; npc.damageType2=3;
        }
        if(i===2) npc.walkDir=5;
        if(i===3) {npc.walkDir=1; npc.runDir=6;}
        npcRenderer.computeInfo(npc);
    }
    const newNpc=new Npc(4321,678); newNpc.active=true; newNpc.jump=true;
    newNpc.coord=CoordGrid.from(3201,2,3208); npcs[newNpc.nid]=newNpc;
    npcMap.zone(3201,2,3208).addNpc(newNpc.nid); npcRenderer.computeInfo(newNpc);
    const npcData=new NpcInfoEncoder().encode(0,npcRenderer,npcs,npcMap,npcPlayer,0,0,false);
    // Independent logical expectations from the fixture inputs above. These do
    // not inspect bytes or invert an engine write transform.
    const old=[['Idle'],['Extended'],['Walk',5,false],['Run',1,6,false],['Remove']];
    const appearanceFields=[Buffer.from(appearanceBytes).toString('hex'),1,2,-1,[261,522,...Array(10).fill(0)],null,[1,2,3,4,5],[100,101,102,103,104,105,null],'72623859790382856',77,1234];
    const playerFields=['PlayerInfo',['Teleport',true,2,51,53,false],5,old,[[23,-2,3,true,true]],[
        [['Old',1],2047,[321,7],'~all player masks',[11,20,29,38,123,234,3],32791,[6411,6413],[456,23,234],appearanceFields,[19,2,67,99],[1029,2,'12ab7f'],[37,3,67,99]],
        [['New',23],6,null,null,null,null,[6403,6417],null,appearanceFields,null,null,null],
    ]];
    const npcFields=['NpcInfo',5,old,[[4321,678,-2,3,true,true]],[
        [['Old',1],255,345,32791,[19,2,67,99],[456,23,234],'all npc masks',[6411,6413],[321,7],[37,3,67,99]],
        [['New',4321],8,null,null,null,null,null,[6403,6417],null,null],
    ]];
    return {payloads:new Map([[90,playerData],[71,npcData]]),expected:new Map([[90,playerFields],[71,npcFields]])};
}
