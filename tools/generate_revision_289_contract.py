#!/usr/bin/env python3
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path('/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java')
client = (SOURCE / 'client.java').read_text()
class17 = (SOURCE / 'Class17.java').read_text()
match = re.search(r'anIntArray209 = new int\[\] \{([^}]*)\}', class17)
assert match is not None
sizes = [int(x.strip()) for x in match.group(1).split(',') if x.strip()][:256]
special = {
  13: ('camera_or_scene_triplet', ['g1 field anInt212', 'g1 field anInt234', 'g1 field anInt360'], 'client.java:2612-2619'),
  46: ('config_signed_short', ['signed g2 value anInt381'], 'client.java:2629-2635'),
  55: ('interface_dual_open', ['g2 interface id anInt377', 'g2 interface id anInt232'], 'client.java:2593-2610'),
  59: ('if_settext', ['g2 component id', 'newline-terminated string method483'], 'client.java:2638-2646'),
  75: ('varp_small', ['g2 variable id', 'signed g1 variable value'], 'client.java:3402-3415'),
  76: ('inventory_partial', ['g2 component/container id', 'repeated gsmart slot', 'g2 item id', 'g1 count or g4 count when count byte is 255'], 'client.java:3477-3494'),
  97: ('varp_large', ['g2 variable id', 'g4 variable value'], 'client.java:3526-3540'),
  107: ('inventory_full', ['g2 component/container id', 'g2 entry count', 'repeated g2 item id', 'g1 count or g4 count when count byte is 255', 'zero-fill remaining component entries'], 'client.java:2972-2990'),
  121: ('logout', ['no payload; calls method104 and returns to login/reset path'], 'client.java:2833-2837'),
  127: ('interface_close_or_set', ['signed g2 interface id', 'method186 for non-negative id', 'stores anInt246'], 'client.java:3393-3400'),
  172: ('varp_bulk_sync', ['no payload; copies current varp values and invokes method229 for changes'], 'client.java:3351-3360'),
  188: ('player_update', ['bit-packed actor count/masks and variable movement/update records', 'final cursor must equal frame length'], 'client.java:2819-2823; client.java:6150-6168'),
  201: ('reset_actors', ['no payload; clears actor animation/target state'], 'client.java:3496-3508'),
  211: ('if_setanim', ['g2 component id', 'signed g2 animation id'], 'client.java:2722-2732'),
  219: ('region_rebuild', ['g2 region base x', 'g2 region base y', 'sets scene loading state'], 'client.java:2999-3022'),
  252: ('if_open_main', ['g2 interface id', 'sets anInt232 and interface state'], 'client.java:2665-2683'),
}
inbound = []
for i, length in enumerate(sizes):
    if i in special:
        name, fields, anchor = special[i]
    else:
        name, fields, anchor = (f'server_opcode_{i:03d}', ['unknown: decode fields from client.java dispatch branch before implementation'], f'client.java:2553-2587 (dispatch; branch tracing pending for opcode {i})')
    inbound.append({'name': name, 'id': i, 'length': length, 'fields': fields,
      'source_anchors': [f'Class17.java:11 (anIntArray209[{i}] packet length)', anchor],
      'confidence': 'verified-length-and-fields' if i in special else 'verified-id-and-length; fields-unknown'})
ids = sorted({int(x) for x in re.findall(r'method465\((\d+)\)', client)})
outbound = []
for i in ids:
    outbound.append({'name': f'client_opcode_{i:03d}', 'id': i, 'length': 'unknown',
      'fields': ['unknown: payload writes are branch-dependent and must be traced from the enclosing client.java method before implementation'],
      'source_anchors': [f'client.java:method465({i}) call sites'],
      'confidence': 'verified-id; length/semantic mapping pending independent trace'})
contract = {'revision': 289, 'source_pin': {
  'repository': 'https://github.com/RuneWiki/openrs2-nonfree.git', 'branch': '289',
  'commit': '0c00ef249546fada67b1f6eb8bbe01ea7c250c95',
  'entry': 'nonfree/client/src/main/java/client.java',
  'inventory': 'FR-vault/docs/research/deob-289-source-inventory.md',
  'artifacts': {'client_source_sha256': 'd03a34d8c965a426993f5a3e812bc566a80a18a91de65e994a257c49a5f5fea6',
                'deob_client_jar_sha256': '3ec08f3e733f0f021d73f180300f00349cb01e4a50803ab5f9efe2af839f70f6',
                'game_cache_pairing': 'unknown'}}, 'inbound': inbound, 'outbound': outbound}
(ROOT / 'docs/revision-289/protocol-289.json').write_text(json.dumps(contract, indent=2) + '\n')
cases = [
 {'name':'fixed_logout_opcode_121','opcode':121,'length_kind':'fixed','frame_hex':'79','payload_hex':'','expected_consumed_length':1,'expected_cursor':0,'expected_result':{'status':'dispatch','state':'method104_reset_then_login','error':None},'source_anchors':['Class17.java:11 (length[121] = 0)','client.java:2553-2587','client.java:2833-2837'],'oracle_derivation':'The primary dispatch branch for opcode 121 consumes no payload, calls method104, clears the opcode and returns false; header consumption is one byte.'},
 {'name':'variable_g2_frame_pending','opcode':47,'length_kind':'g2','frame_hex':'2f0005','payload_hex':'','declared_payload_length':5,'expected_consumed_length':3,'expected_cursor':0,'expected_result':{'status':'pending','state':'frame_length_read','error':None},'source_anchors':['Class17.java:11 (length[47] = -2)','client.java:2553-2587'],'oracle_derivation':'The primary framing code reads the two-byte big-endian length, then returns false when available payload bytes are fewer than the declared five; no payload is consumed.'},
 {'name':'variable_g2_frame_complete','opcode':47,'length_kind':'g2','frame_hex':'2f00020102','payload_hex':'0102','declared_payload_length':2,'expected_consumed_length':5,'expected_cursor':2,'expected_result':{'status':'dispatch','state':'payload_available','error':None},'source_anchors':['Class17.java:11 (length[47] = -2)','client.java:2574-2589'],'oracle_derivation':'The primary framing code consumes opcode, g2 length, and exactly two payload bytes once local16 is at least anInt361.'},
 {'name':'inventory_full_g2_count','opcode':107,'length_kind':'g2','frame_hex':'6b000a00030002000102000303','payload_hex':'00030002000102000303','declared_payload_length':10,'expected_consumed_length':13,'expected_cursor':10,'expected_result':{'status':'dispatch','state':'inventory_full','component_id':3,'entry_count':2,'entries':[{'item_id':1,'count':2},{'item_id':3,'count':3}]},'source_anchors':['Class17.java:11 (length[107] = -2)','client.java:2972-2990'],'oracle_derivation':'The primary branch reads g2 component 3, g2 count 2, then each g2 item and g1 count; neither count uses the 255 extension in this vector.'},
 {'name':'inventory_partial_gsmart_slot','opcode':76,'length_kind':'g2','frame_hex':'4c000600030100010000','payload_hex':'000301000100','declared_payload_length':6,'expected_consumed_length':9,'expected_cursor':6,'expected_result':{'status':'dispatch','state':'inventory_partial','component_id':3,'entries':[{'slot':1,'item_id':1,'count':0}]},'source_anchors':['Class17.java:11 (length[76] = -2)','client.java:3477-3494','Class1_Sub1_Sub3.java:402-405'],'oracle_derivation':'The primary branch reads component 3, then method490 slot 1 (one-byte smart), g2 item 1 and g1 count 0; the trailing zero is intentionally outside the declared frame and is not consumed.'},
 {'name':'actor_update_empty_payload_cursor','opcode':188,'length_kind':'g2','frame_hex':'bc0000','payload_hex':'','declared_payload_length':0,'expected_consumed_length':3,'expected_cursor':0,'expected_result':{'status':'dispatch','state':'actor_update_requires_source_structure','error':'actor decoder outcome is blocked without a traced bitstream'},'source_anchors':['Class17.java:11 (length[188] = -2)','client.java:2819-2823','client.java:6150-6168'],'oracle_derivation':'Only the dispatch call and final cursor equality are source-proven here; no invented actor bitstream is asserted.'},
 {'name':'region_scene_base','opcode':219,'length_kind':'fixed','frame_hex':'db01020304','payload_hex':'01020304','expected_consumed_length':5,'expected_cursor':4,'expected_result':{'status':'dispatch','state':'region_rebuild_pending','region_x':258,'region_y':772},'source_anchors':['Class17.java:11 (length[219] = 4)','client.java:2999-3022'],'oracle_derivation':'The primary branch reads two big-endian g2 coordinates: 0x0102=258 and 0x0304=772, then sets scene loading state.'},
 {'name':'widget_text_newline','opcode':59,'length_kind':'g2','frame_hex':'3b0005000568690a','payload_hex':'000568690a','declared_payload_length':5,'expected_consumed_length':8,'expected_cursor':5,'expected_result':{'status':'dispatch','state':'widget_text','component_id':5,'text':'hi'},'source_anchors':['Class17.java:11 (length[59] = -2)','client.java:2638-2646','Class1_Sub1_Sub3.java:308-314'],'oracle_derivation':'The primary branch reads g2 component 5, then method483 consumes bytes through newline 0x0a and returns the preceding UTF-8/byte string hi.'},
 {'name':'reset_actors_empty','opcode':201,'length_kind':'fixed','frame_hex':'c9','payload_hex':'','expected_consumed_length':1,'expected_cursor':0,'expected_result':{'status':'dispatch','state':'actor_targets_reset','error':None},'source_anchors':['Class17.java:11 (length[201] = 0)','client.java:3496-3508'],'oracle_derivation':'The primary branch has no reads and iterates both actor arrays to set anInt1003 to -1.'},
]
fixture = ROOT / 'crates/client/tests/fixtures/revision_289/manifest.json'
fixture.parent.mkdir(parents=True, exist_ok=True)
fixture.write_text(json.dumps({'revision':289, 'cases':cases}, indent=2) + '\n')
print(f'generated {len(inbound)} inbound and {len(outbound)} outbound rows')
