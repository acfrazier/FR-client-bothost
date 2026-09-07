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
  188: ('player_update', ['bit-packed actor count/masks and variable movement/update records; final cursor must equal frame length']),
  172: ('inventory_full', ['g2 component/container id; g2 slot count; repeated gsmart item ids/counts per slot']),
  76: ('inventory_partial', ['g2 component/container id; repeated g2 slot, g2 item id, g1 count or g4 count when 255']),
  55: ('region_rebuild', ['g2 region base coordinates; map/scene rebuild state']),
  13: ('varp_small', ['g1 variable id/value']),
  46: ('varp_large', ['g2 variable id/value']),
  59: ('if_settext', ['g2 component id; newline-terminated string']),
  252: ('if_open', ['g2 interface id; interface stack/reset state']),
  211: ('if_open_main', ['g2 component/interface id and interface state']),
  127: ('logout', ['g1 signed logout/reset reason']),
  201: ('reset_actors', ['no payload; clears actor animation/target state']),
}
inbound = []
for i, length in enumerate(sizes):
    name, fields = special.get(i, (f'server_opcode_{i:03d}', ['unknown: decode fields from client.java dispatch branch before implementation']))
    inbound.append({'name': name, 'id': i, 'length': length, 'fields': fields,
      'source_anchors': [f'Class17.java:11 (anIntArray209[{i}] packet length)',
                         f'client.java:2593-3510 (dispatch branch for opcode {i})'],
      'confidence': 'verified-length; fields-source-derived' if i in special else 'verified-id-and-length; fields-unknown'})
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
 {'name':'fixed_empty_logout_reset','hex':'7f','source_anchors':['client.java:2553-2587','client.java:3510-3550'],'expected_consumed_length':1,'expected_decoded_state':'opcode byte only; no payload; reset/error depends on the selected 289 table entry','oracle_derivation':'One byte is the complete fixed-frame header in the source framing rule; no Rust decoder used.'},
 {'name':'variable_two_byte_frame_length','hex':'aa0005','source_anchors':['client.java:2574-2587'],'expected_consumed_length':3,'expected_decoded_state':'opcode 0xaa followed by big-endian declared length 5; truncated payload must remain pending/rejected, never partially accepted','oracle_derivation':'The Java dispatcher reads g2 for a -2 length and compares available bytes before reading the payload.'},
 {'name':'inventory_g2_gsmart_slots','hex':'ac00030002018001','source_anchors':['client.java:3351-3360','docs/revision-289/host-script-evidence.md:54'],'expected_consumed_length':8,'expected_decoded_state':'container 3; count 2; two slot values decode as 1 and 1 from one-byte and two-byte gsmart encodings','oracle_derivation':'Manual big-endian g2 and Rune smart rule: values below 128 use one byte; values with high bit set use g2 minus 0x8000. Derived from the encoding definition before any client implementation.'},
 {'name':'actor_update_empty_mask_reject','hex':'bc0000','source_anchors':['client.java:2819-2822','client.java:6150-6168'],'expected_consumed_length':3,'expected_decoded_state':'player-update branch receives zero-length actor payload and rejects because the final position cannot satisfy the required update structure','oracle_derivation':'The source calls actor update then requires packet cursor == declared size; this case deliberately omits required bit fields.'},
 {'name':'actor_update_mask_bits','hex':'bc0101aa','source_anchors':['client.java:2819-2822','client.java:6150-6168'],'expected_consumed_length':4,'expected_decoded_state':'one actor-update record with an explicit update-mask byte 0xaa; any missing mask payload is rejected at the declared frame boundary','oracle_derivation':'The mask byte is selected by hand to exercise bit-preserving framing; expected rejection follows the source final-cursor invariant, independently of Rust.'},
 {'name':'region_scene_base','hex':'3701020304','source_anchors':['client.java:2593-2610','client.java:8462-8469'],'expected_consumed_length':5,'expected_decoded_state':'region packet decodes two big-endian coordinates and leaves scene rebuild pending; four plane scene arrays are cleared on reset','oracle_derivation':'Coordinates are hand-calculated from the source g2 reads; reset clearing is quoted source behavior, not Rust behavior.'},
 {'name':'widget_text_newline','hex':'3b000501020a6869','source_anchors':['client.java:2638-2646'],'expected_consumed_length':8,'expected_decoded_state':'component id 5 then text bytes 01 02 until newline; malformed text remains a protocol error','oracle_derivation':'Manual g2 plus newline-terminated string rule from the deob packet reader; no Rust decoder used.'},
 {'name':'logout_reset_state','hex':'7f00','source_anchors':['client.java:8408-8503','client.java:8540-8553'],'expected_consumed_length':2,'expected_decoded_state':'logout/reset response clears packet buffers, actor arrays and 4x104x104 scene references; response 15 keeps login state but resets frame buffers','oracle_derivation':'State list is transcribed from the Java reset branches; bytes are public-safe placeholders for the selected response path.'},
]
fixture = ROOT / 'crates/client/tests/fixtures/revision_289/manifest.json'
fixture.parent.mkdir(parents=True, exist_ok=True)
fixture.write_text(json.dumps({'revision':289, 'cases':cases}, indent=2) + '\n')
print(f'generated {len(inbound)} inbound and {len(outbound)} outbound rows')
