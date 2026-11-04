#!/usr/bin/env node
// Executes unchanged engine encoders and original Java Packet readers. This is
// a synthetic wire corpus, not a captured login or an execution of Client.readPacket.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';

const args = process.argv.slice(2);
function option(name) {
    const index = args.indexOf(name);
    if (index < 0 || !args[index + 1]) throw new Error(`required: ${name}`);
    return path.resolve(args[index + 1]);
}
const engine = option('--engine-root');
const java = option('--java-root');
const work = option('--work-dir');
const check = args.includes('--check');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.join(root, 'crates/client/tests/fixtures/revision_377/packets.json');
fs.mkdirSync(work, { recursive: true });
const require = createRequire(path.join(engine, 'package.json'));
const { register } = await import(pathToFileURL(require.resolve('tsx/esm/api')).href);
register();
const load = async relative => (await import(pathToFileURL(path.join(engine, relative)).href)).default;
const Packet = await load('src/io/Packet.ts');
const Component = await load('src/cache/config/Component.ts');
const codecRoot = 'src/network/game/server/codec';
const javaPath = 'src/main/java/jagex2/client/Client.java';
const source = fs.readFileSync(path.join(java, javaPath), 'utf8');
const readStart = source.indexOf('public boolean readPacket()');
const zoneStart = source.indexOf('public void readZonePacket(');
const readBody = source.slice(readStart, zoneStart);
const handlerIds = [...new Set([...readBody.matchAll(/this\.ptype == (\d+)/g)].map(m => Number(m[1])))].sort((a,b) => a-b);
const deferred = [53,71,90,222];
const covered = handlerIds.filter(id => !deferred.includes(id));
const sources = {};
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function pin(base, relative, group) {
    sources[`${group}/${relative}`] = digest(fs.readFileSync(path.join(base, relative)));
}
for (const relative of [javaPath, 'src/main/java/jagex2/io/Packet.java', 'src/main/java/jagex2/io/Protocol.java']) pin(java, relative, 'java');
for (const relative of ['src/io/Packet.ts', 'src/network/game/server/ServerGameProt.ts', 'src/network/game/server/ServerGameZoneProt.ts', 'src/network/game/server/ServerGameMessageEncoder.ts', 'src/cache/config/Component.ts']) pin(engine, relative, 'engine');
for (const relative of ['src/cache/wordenc/WordEnc.ts', 'src/wordenc/WordPack.ts', 'src/engine/CoordGrid.ts']) pin(engine, relative, 'engine');
function block(start) {
    const open = source.indexOf('{', start);
    let depth = 1, end = open + 1;
    while (depth && end < source.length) {
        if (source[end] === '{') depth++;
        if (source[end] === '}') depth--;
        end++;
    }
    if (depth) throw new Error('unbalanced Java block');
    return { text: source.slice(open + 1, end - 1), line: source.slice(0, start).split('\n').length, end: source.slice(0, end).split('\n').length };
}
function javaBlock(id) {
    const zone = source.indexOf(`if (arg2 == ${id})`, zoneStart);
    if (zone >= 0 && zone < source.indexOf('public void appendLoc(', zoneStart)) return block(zone);
    const start = source.indexOf(`if (this.ptype == ${id})`, readStart);
    if (start < 0 || start >= zoneStart) throw new Error(`missing Java handler ${id}`);
    return block(start);
}
function javaReads(id, encoded) {
    const b = javaBlock(id);
    let reads = [...b.text.matchAll(/(?:this\.in|arg0)\.(g\w+)\(\)/g)].map(m => m[1]);
    // Java's original control flow, expanded for the concrete synthetic case.
    // Client.java:7499-7536 ignores reserved HINT_ARROW tails.
    if (id === 199) reads = encoded[0] >= 2 && encoded[0] <= 6 ? ['g1','g2','g2','g1'] : encoded[0] === 1 || encoded[0] === 10 ? ['g1','g2','rest'] : ['g1','rest'];
    if (id === 135) reads.push('rest'); // Client.java:7918-7959; packed chat stays opaque.
    // Expand Java's loops from their own read order, never from engine writes.
    // Client.java:7793-7812,8065-8085. Only branch scheduling is done here;
    // the unmodified Java Packet still performs every recorded primitive read.
    if (id === 206 || id === 134) {
        const schema = id === 206 ? ['g2','g2','g2_alt3','g1_alt2','g4_alt1'] : ['g2','gsmarts','g2','g1','g4'];
        if (JSON.stringify(reads) !== JSON.stringify(schema)) throw new Error(`changed Java inventory schema ${id}`);
        reads = id === 206 ? ['g2','g2'] : ['g2'];
        let pos = id === 206 ? 4 : 2;
        const count = id === 206 ? encoded.readUInt16BE(2) : Infinity;
        for (let entry = 0; id === 206 ? entry < count : pos < encoded.length; entry++) {
            if (id === 134) { reads.push('gsmarts'); pos += encoded[pos] < 128 ? 1 : 2; }
            reads.push(id === 206 ? 'g2_alt3' : 'g2', id === 206 ? 'g1_alt2' : 'g1');
            const countByte = encoded[pos + 2];
            pos += 3;
            if ((id === 206 ? -countByte & 255 : countByte) === 255) {
                reads.push(id === 206 ? 'g4_alt1' : 'g4');
                pos += 4;
            }
        }
        if (pos !== encoded.length) throw new Error(`invalid inventory loop boundary ${id}`);
    }
    if (id === 226) {
        if (encoded.length % 8 !== 0) throw new Error('invalid ignore-list loop boundary');
        reads = Array(encoded.length / 8).fill('g8');
    }
    if (id === 183) reads = ['g1','g1_alt1','rest']; // Children also have individual corpus cases.
    return { reads, anchor:`${javaPath}:${b.line}-${b.end}` };
}
// Compare Java-decoded logical scalars with the original encoder arguments.
// Narrow by the JAVA read's signedness/width, not by the engine writer name;
// wrong engine transforms or field order must therefore fail this comparison.
function javaWidth(method, value) {
    if (method === 'g8') return BigInt.asIntN(64, BigInt(value)).toString();
    if (method === 'gjstr') return String(value);
    if (method === 'gsmarts') return Number(value);
    if (['g1b','g1b_alt1','g1b_alt2','g1b_alt3'].includes(method)) return Number(value) << 24 >> 24;
    if (['g1','g1_alt1','g1_alt2','g1_alt3'].includes(method)) return Number(value) & 255;
    if (['g2b','g2b_alt1','g2b_alt2'].includes(method)) return Number(value) << 16 >> 16;
    if (['g2','g2_alt1','g2_alt2','g2_alt3'].includes(method)) return Number(value) & 65535;
    if (method === 'g3_alt3') return Number(value) & 16777215;
    if (['g4','g4_alt1','g4_alt3'].includes(method)) return Number(value) | 0;
    throw new Error(`unsupported Java scalar ${method}`);
}
const values = {
    component: 0x1234, mainComponent: 0x1235, sideComponent: 0x1236, overlayComponent: 0x1237,
    main: 0x1235, side: 0x1236,
    model: 0x2345, npc: 0x3456, obj: 0x4567, scale: 150, sequence: 321,
    seq: 321, loc: 456, oldCount: 1234, newCount: 2345,
    x: -123, y: -456, z: 35, xan: 0x123, yan: 0x234, zoom: 0x345,
    xAngleSpeed: 0x8123, yAngleSpeed: 0xfedc, colour: 0x4567,
    hide: true, hidden: true, tab: 3, text: 'Fixture text',
    varp: 0x123, value: -123456789, stat: 3, level: 67, exp: 1234560,
    energy: 7300, weight: -321, kg: -321, members: true, uid: 0x456, ticks: 123,
    name: 0x0102030405060708n, from: 0x0102030405060708n, names: [0x0102030405060708n,0x1122334455667788n],
    nodeId: 42, msg: 'hello', messageId: 0x12345678, staffModLevel: 3,
    publicChat: 1, privateChat: 2, tradeDuel: 0, status: 2, minimapType: 2,
    op: 4, primary: 0, synth: 456, loops: 3, delay: 7, id: 234,
    type: 4, nid: 456, playerSlot: 321, height: 567, rate: 23, rate2: 101,
    axis: 4, random: 17, amplitude: 19,
    daysSincePasswordChange: 123, currentDay: 2345, unreadMessageCount: 7,
    previousLoginDay: 2344, membersCreditDays: 29, lastLoginIp: 0x7f000001, daysSinceRecoveryChange: 456,
    zoneX: 400, zoneZ: 401, originX: 3200, originZ: 3200, data: new Uint8Array(),
    coord: 0x35, shape: 10, angle: 3, locId: 456, count: 2345,
    receiverId: 321, spotanim: 456, target: -321, srcHeight: 7, dstHeight: 8,
    startDelay: 123, endDelay: 234, peak: 45, arc: 67, startCycle: 123, endCycle: 234,
    srcX: 50, srcZ: 60, dstX: 48, dstZ: 63, west: 48, east: 53, south: 59, north: 64,
};
const inventoryObjects = new Map([[0,{id:123,count:2}],[128,{id:234,count:65537}]]);
const inventory = { capacity:129, get: slot => inventoryObjects.get(slot) ?? null };
// Only a process-local synthetic inventory definition is supplied. Original
// Component.get and encoder logic remain unchanged. No engine is started.
Component.components[values.component] = { width:129, height:1 };
const cases = [];
function safe(value) {
    if (typeof value === 'bigint') return value.toString();
    if (value instanceof Uint8Array) return Buffer.from(value).toString('hex');
    return value;
}
const codecs = fs.readdirSync(path.join(engine, codecRoot)).filter(name => name.endsWith('Encoder.ts')).sort();
for (const file of codecs) {
    const Encoder = await load(`${codecRoot}/${file}`);
    const encoder = new Encoder();
    const id = encoder.prot.id;
    if (!covered.includes(id)) continue;
    pin(engine, `${codecRoot}/${file}`, 'engine');
    const input = { ...values, inv:inventory, slots:[0,1,128] };
    if (id === 182) input.value = -37;
    if (id === 157) input.value = 'Trade';
    if (id === 249) input.delay = 0x123456;
    if (id === 199 || id === 3 || id === 167) { input.x = 34; input.z = 35; }
    const fields = {};
    const message = new Proxy(input, { get(target, key) {
        if (!(key in target)) throw new Error(`missing fixture field ${String(key)} for ${file}`);
        const value = target[key];
        if (key !== 'inv') fields[key] = safe(value);
        return value;
    }});
    const packet = new Packet(new Uint8Array(65535));
    const writes = [];
    const buffer = new Proxy(packet, { get(target, key) {
        const value = target[key];
        if (typeof value !== 'function') return value;
        return (...args) => {
            if (String(key).startsWith('p')) writes.push({ method:String(key), args:args.map(safe) });
            return value.apply(target, args);
        };
    }});
    encoder.encode(buffer, message);
    const payload = packet.data.slice(0,packet.pos);
    const j = javaReads(id,Buffer.from(payload));
    if (encoder.prot.length >= 0 && encoder.prot.length !== payload.length) throw new Error(`engine fixed length mismatch for ${file}`);
    const estimated_length = typeof encoder.test === 'function' ? encoder.test(message) : null;
    cases.push({ name:file.replace('Encoder.ts',''), opcode:id, origin:'original-engine-encoder', fields, writes, declared_length:encoder.prot.length, estimated_length, payload_hex:Buffer.from(payload).toString('hex'), ...j });
}
// These Java handlers have no corresponding engine encoder in the selected tree.
for (const [opcode,name,payload_hex] of [[6,'PNameDialog',''],[41,'SoundArea','3501c873']]) {
    if (cases.some(row => row.opcode === opcode)) throw new Error(`engine now supplies ${name}; remove Java-only case`);
    const j=javaReads(opcode,Buffer.from(payload_hex,'hex'));
    cases.push({ name, opcode, origin:'Java-read-order literal', writes:[], payload_hex, ...j });
}
cases.sort((a,b) => a.opcode-b.opcode);
if (JSON.stringify(cases.map(c=>c.opcode)) !== JSON.stringify(covered)) throw new Error('incomplete or duplicate corpus');
const classes=path.join(work,'classes');
fs.mkdirSync(classes,{recursive:true});
execFileSync('javac',['-d',classes,'-sourcepath',path.join(java,'src/main/java'),path.join(root,'tools/Revision377PacketOracle.java')],{stdio:['ignore','pipe','pipe']});
const input=cases.map(row=>`${row.payload_hex}\t${row.reads.join(',')}\n`).join('');
const javaOutput=execFileSync('java',['-Dfile.encoding=UTF-8','-cp',classes,'Revision377PacketOracle'],{input,encoding:'utf8'}).trim().split('\n').map(line=>JSON.parse(line));
for (let i=0;i<cases.length;i++) {
    const row=cases[i], result=javaOutput[i];
    if (result.consumed !== row.payload_hex.length/2) throw new Error(`Java under/over-consumed ${row.name}: ${result.consumed}`);
    row.java_values=result.values;
    row.consumed=result.consumed;
    row.engine_scalar_checks = 0;
    if (row.origin === 'original-engine-encoder') {
        for (let j=0;j<row.reads.length && row.reads[j] !== 'rest';j++) {
            if (!row.writes[j]) throw new Error(`missing original encoder write ${row.name}:${j}`);
            const expected=javaWidth(row.reads[j],row.writes[j].args[0]);
            if (expected !== result.values[j]) throw new Error(`engine/Java value mismatch ${row.name}:${j}: expected ${expected}, got ${result.values[j]}`);
            row.engine_scalar_checks++;
        }
    }
}
const fixture={ revision:377, method:'Executed original engine encoders and untouched Java Packet readers; Client read control flow expanded for synthetic inputs, not a live capture.', source_sha256:sources, implemented_java_opcodes:handlerIds, covered_opcodes:covered, deferred_opcodes:deferred, cases };
const bytes=JSON.stringify(fixture,(_key,value)=>typeof value==='bigint'?value.toString():value,2)+'\n';
if (check) {
    if (fs.readFileSync(output,'utf8') !== bytes) throw new Error('stale original-engine/Java packet corpus');
} else fs.writeFileSync(output,bytes);
console.log(`PASS: ${covered.length}/${handlerIds.length} Java inbound handlers; ${cases.filter(c=>c.origin==='original-engine-encoder').length} original engine encoders; ${cases.length} complete original-Java primitive traces; ${cases.reduce((sum,row)=>sum+row.engine_scalar_checks,0)} engine-to-Java logical scalar comparisons; ${deferred.length} explicit deferred handlers`);
