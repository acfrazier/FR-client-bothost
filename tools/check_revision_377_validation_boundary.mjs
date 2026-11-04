#!/usr/bin/env node
// Compile the real additive packet module without changing shared registrations.
// Positive and negative probes prove callers cannot borrow an unvalidated op or
// bypass the root through child decoders. No executable is linked or launched.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const at=process.argv.indexOf('--work-dir');
if(at<0||!process.argv[at+1])throw new Error('required: --work-dir');
const work=path.resolve(process.argv[at+1]);fs.mkdirSync(work,{recursive:true});
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const packet=path.join(root,'crates/client/src/client/packet_377.rs');
const sizes=path.join(root,'crates/client/src/io/server_prot_sizes_377.inc.rs');
const prelude=`#[path=${JSON.stringify(sizes)}] mod sizes;\nmod io { pub use crate::sizes::SERVER_PROT_SIZES_377; }\n#[path=${JSON.stringify(packet)}] mod packet_377;\n`;
const probes=[
    ['validated',`pub fn check(p:packet_377::DecodedPacket377<'_>,c:&impl packet_377::PacketContext377){let validated=p.validate(c).unwrap();let _=validated.operation();let _=validated.into_operation();}`,true,null],
    ['unvalidated-borrow',`pub fn check(p:packet_377::DecodedPacket377<'_>){let _=p.operation();}`,false,'E0599'],
    ['unvalidated-move',`pub fn check(p:packet_377::DecodedPacket377<'_>){let _=p.into_operation();}`,false,'E0599'],
    ['interface-bypass',`pub fn check(p:&mut packet_377::PacketReader377<'_>){let _=packet_377::interface_packet_377::InterfaceOperation377::decode(6,p);}`,false,'E0624'],
    ['zone-bypass',`pub fn check(p:&mut packet_377::PacketReader377<'_>){let _=packet_377::zone_377::ZoneFrame377::decode(75,p);}`,false,'E0624'],
    ['player-bypass',`pub fn check(p:&mut packet_377::PacketReader377<'_>){let _=packet_377::player_info_377::PlayerInfo377::decode(p);}`,false,'E0624'],
    ['npc-bypass',`pub fn check(p:&mut packet_377::PacketReader377<'_>){let _=packet_377::npc_info_377::NpcInfo377::decode(p);}`,false,'E0624'],
    ['rebuild-bypass',`pub fn check(p:&mut packet_377::PacketReader377<'_>){let _=packet_377::rebuild_377::Rebuild377::decode(222,p);}`,false,'E0624'],
];
const results=[];
for(const [name,body,success,error] of probes){
    const source=path.join(work,`${name}.rs`);fs.writeFileSync(source,prelude+body+'\n');
    const args=['--crate-name','validation_boundary','--crate-type','lib','--edition','2021','--cap-lints','allow',source,'--out-dir',work];
    const result=spawnSync('rustc',args,{encoding:'utf8'});
    fs.writeFileSync(path.join(work,`${name}.log`),result.stdout+result.stderr);
    if(result.error)throw result.error;
    if((result.status===0)!==success||(!success&&!result.stderr.includes(`error[${error}]`)))throw new Error(`unexpected compiler result for ${name}: ${result.stderr}`);
    results.push({name,argv:['rustc',...args],exit:result.status,expected_error:error});
}
fs.writeFileSync(path.join(work,'results.json'),JSON.stringify(results,null,2)+'\n');
console.log(`PASS: validated handoff compiles; ${results.length-1} unvalidated/bypass probes fail with the expected compiler error`);
