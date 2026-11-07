use client::client::{Client, ClientConfig, ClientPlayer};
use client::config::{IfType, IfTypeMut};
use client::io::{ClientRevision, Packet, ServerProt, ServerProt289};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

const UNCHANGED_ITERATIONS: usize = 128;

struct Counting;

thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

// SAFETY: All allocator calls are forwarded unchanged to `System`; only a
// thread-local event counter is updated while a measurement is active.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        // SAFETY: The caller supplies a valid layout that is forwarded intact.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        // SAFETY: The caller supplies a valid layout that is forwarded intact.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        COUNT.with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        // SAFETY: The pointer and original layout are forwarded unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The pointer and original layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|count| count.set(Some(0)));
    f();
    COUNT.with(|count| count.replace(None).expect("allocation counter is active"))
}

#[derive(Clone, Copy)]
struct InboundOpcodes {
    player_info: i32,
    npc_info: i32,
    runenergy: i32,
    zone_obj_del: i32,
    inventory_full: i32,
    varp_small: i32,
    inventory_payload: &'static [u8],
}

fn inbound_opcodes(revision: ClientRevision) -> InboundOpcodes {
    match revision {
        ClientRevision::R274 => InboundOpcodes {
            player_info: ServerProt::PLAYER_INFO,
            npc_info: ServerProt::NPC_INFO,
            runenergy: ServerProt::UPDATE_RUNENERGY,
            zone_obj_del: ServerProt::OBJ_DEL,
            inventory_full: ServerProt::UPDATE_INV_FULL,
            varp_small: ServerProt::VARP_SMALL,
            inventory_payload: &[0, 3, 1, 0, 1, 2],
        },
        ClientRevision::R289 => InboundOpcodes {
            player_info: ServerProt289::PLAYER_INFO,
            npc_info: ServerProt289::NPC_INFO,
            runenergy: ServerProt289::UPDATE_RUNENERGY,
            zone_obj_del: ServerProt289::ZONE_OBJ_DEL,
            inventory_full: ServerProt289::UPDATE_INV_FULL,
            varp_small: ServerProt289::VARP_SMALL,
            inventory_payload: &[0, 3, 0, 1, 0, 1, 2],
        },
    }
}

fn client(revision: ClientRevision) -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        revision,
    );
    c.ingame = true;
    c.ptype = -1;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.players[2047] = Some(Box::default());
    c.zone_update_x = 8;
    c.zone_update_z = 16;
    c.var.resize(4, 0);
    c.var_serv.resize(4, 0);

    let inventory = IfTypeMut {
        link_obj_type: Some(vec![0; 4]),
        link_obj_number: Some(vec![0; 4]),
        ..Default::default()
    };
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        slots.resize_with(4, || None);
        slots[3] = Some(Arc::new(inventory));
    }
    {
        let table = Arc::make_mut(&mut c.ifaces);
        table.resize_with(4, || None);
        table[3] = Some(Box::new(IfType::default()));
    }
    c
}

fn player_info_frame() -> Vec<u8> {
    // Production PLAYER_INFO appearance + face-entity mask, as used by both
    // revision fixtures. Replaying it exercises the warmed actor apply path.
    let mut frame = vec![0x80, 0x1f, 0xfc, 0x05, 44];
    let mut appearance = [0; 44];
    appearance[19..33].fill(0xff);
    frame.extend_from_slice(&appearance);
    frame.extend_from_slice(&[0x12, 0x34]);
    frame
}

fn dispatch(c: &mut Client, opcode: i32, packet: &mut Packet) {
    packet.pos = 0;
    packet.clear_frame_end();
    packet.set_frame_end(packet.length());
    c.psize = packet.length() as i32;
    c.handle_packet(opcode, packet);
}

fn warmed_dispatch_allocations(
    revision: ClientRevision,
    c: &mut Client,
    label: &str,
    opcode: i32,
    packet: &mut Packet,
) -> usize {
    dispatch(c, opcode, packet);
    dispatch(c, opcode, packet);
    let baseline = allocations(|| dispatch(c, opcode, packet));
    let mut samples = [0; UNCHANGED_ITERATIONS];
    for sample in &mut samples {
        *sample = allocations(|| dispatch(c, opcode, packet));
    }
    assert!(
        samples.iter().all(|&sample| sample == baseline),
        "{revision:?} {label} allocation count changed after warm-up: baseline={baseline}, samples={samples:?}"
    );
    let first_half: usize = samples[..UNCHANGED_ITERATIONS / 2].iter().sum();
    let second_half: usize = samples[UNCHANGED_ITERATIONS / 2..].iter().sum();
    assert_eq!(
        first_half, second_half,
        "{revision:?} {label} allocation count grew across unchanged iterations"
    );
    eprintln!("{revision:?} inbound {label}: {baseline} allocations/dispatch");
    baseline
}

/// Shared revision-parametric inbound allocation body. A later revision adds
/// its opcode/payload row to `inbound_opcodes` and calls this same helper.
fn assert_revision_inbound_allocation_gate(revision: ClientRevision) -> [usize; 6] {
    let opcodes = inbound_opcodes(revision);
    let mut c = client(revision);
    let mut player = Packet::new(player_info_frame());
    let mut npc = Packet::new(vec![0]);
    let mut misc = Packet::new(vec![75]);
    let mut zone = Packet::new(vec![0x12, 0x80, 0]);
    let mut inventory = Packet::new(opcodes.inventory_payload.to_vec());
    let mut protocol = Packet::new(vec![0, 3, 42]);

    let counts = [
        warmed_dispatch_allocations(revision, &mut c, "player actor", opcodes.player_info, &mut player),
        warmed_dispatch_allocations(revision, &mut c, "npc actor", opcodes.npc_info, &mut npc),
        warmed_dispatch_allocations(revision, &mut c, "misc runenergy", opcodes.runenergy, &mut misc),
        warmed_dispatch_allocations(revision, &mut c, "zone object delete", opcodes.zone_obj_del, &mut zone),
        warmed_dispatch_allocations(revision, &mut c, "cache inventory", opcodes.inventory_full, &mut inventory),
        warmed_dispatch_allocations(revision, &mut c, "protocol varp", opcodes.varp_small, &mut protocol),
    ];
    assert!(c.ingame, "all warmed inbound fixtures must remain valid");
    counts
}

#[test]
fn warmed_inbound_decode_and_apply_allocation_gates_cover_each_revision() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        let counts = assert_revision_inbound_allocation_gate(revision);
        let expected = match revision {
            ClientRevision::R274 => [7, 0, 0, 0, 1, 0],
            ClientRevision::R289 => [14, 0, 0, 1, 1, 0],
        };
        assert_eq!(
            counts, expected,
            "{revision:?} warmed inbound allocations changed by event family"
        );
        eprintln!("{revision:?} inbound allocation baseline: {counts:?}");
    }
}
