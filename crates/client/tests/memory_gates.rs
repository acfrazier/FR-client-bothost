use client::client::{Client, ClientConfig, ClientPlayer};
use client::config::{Cache, IfType, IfTypeMut};
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
    zone_payload: &'static [u8],
    varp_payload: &'static [u8],
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
            zone_payload: &[0x12, 0x80, 0],
            varp_payload: &[0, 3, 42],
        },
        ClientRevision::R289 => InboundOpcodes {
            player_info: ServerProt289::PLAYER_INFO,
            npc_info: ServerProt289::NPC_INFO,
            runenergy: ServerProt289::UPDATE_RUNENERGY,
            zone_obj_del: ServerProt289::ZONE_OBJ_DEL,
            inventory_full: ServerProt289::UPDATE_INV_FULL,
            varp_small: ServerProt289::VARP_SMALL,
            inventory_payload: &[0, 3, 0, 1, 0, 1, 2],
            zone_payload: &[0x12, 0x80, 0],
            varp_payload: &[0, 3, 42],
        },
        ClientRevision::R377 => InboundOpcodes {
            player_info: 90,
            npc_info: 71,
            runenergy: 125,
            zone_obj_del: 208,
            inventory_full: 206,
            varp_small: 182,
            // Java Client.java:8065-8085: component, count, LE biased ID,
            // then negated quantity. The item and component match the fixture.
            inventory_payload: &[0, 3, 0, 1, 0x81, 0, 0xfe],
            // Java Client.java:7553-7568 and 7325-7340.
            zone_payload: &[0, 0x80, 0x92],
            varp_payload: &[0, 0x83, 86],
        },
    }
}

fn client(revision: ClientRevision) -> Client {
    let config = ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    };
    let mut c = if revision == ClientRevision::R377 {
        let mut cache = Cache::default();
        cache.objs.push(Default::default());
        cache.varps.resize_with(4, Default::default);
        Client::from_shared_with_revision(
            config,
            Arc::new(cache),
            Arc::new(Vec::new()),
            Vec::new(),
            revision,
        )
    } else {
        Client::new_with_revision(config, revision)
    };
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

fn player_info_frame(revision: ClientRevision) -> Vec<u8> {
    // Production PLAYER_INFO appearance + face-entity mask. R377 reads the
    // transformed target before its reversed appearance's three-byte prefix.
    if revision == ClientRevision::R377 {
        let mut frame = vec![0x80, 0x1f, 0xfc, 0x05, 0x12, 0xb4, 45];
        let mut appearance = [0; 45];
        appearance[20..34].fill(0xff);
        appearance[41] = 1;
        frame.extend(appearance.into_iter().rev());
        return frame;
    }
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
    assert!(
        c.ingame,
        "{revision:?} {label} fixture must warm successfully"
    );
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
    assert!(c.ingame, "{revision:?} {label} fixture must remain valid");
    baseline
}

/// Shared revision-parametric inbound allocation body. Every case calls the
/// production handler entrypoint used by live input, including R377 validation.
fn assert_revision_inbound_allocation_gate(revision: ClientRevision) -> [usize; 6] {
    let opcodes = inbound_opcodes(revision);
    let mut c = client(revision);
    let mut player = Packet::new(player_info_frame(revision));
    let mut npc = Packet::new(vec![0]);
    let mut misc = Packet::new(vec![75]);
    let mut zone = Packet::new(opcodes.zone_payload.to_vec());
    let mut inventory = Packet::new(opcodes.inventory_payload.to_vec());
    let mut protocol = Packet::new(opcodes.varp_payload.to_vec());

    let counts = [
        warmed_dispatch_allocations(
            revision,
            &mut c,
            "player actor",
            opcodes.player_info,
            &mut player,
        ),
        warmed_dispatch_allocations(revision, &mut c, "npc actor", opcodes.npc_info, &mut npc),
        warmed_dispatch_allocations(
            revision,
            &mut c,
            "misc runenergy",
            opcodes.runenergy,
            &mut misc,
        ),
        warmed_dispatch_allocations(
            revision,
            &mut c,
            "zone object delete",
            opcodes.zone_obj_del,
            &mut zone,
        ),
        warmed_dispatch_allocations(
            revision,
            &mut c,
            "cache inventory",
            opcodes.inventory_full,
            &mut inventory,
        ),
        warmed_dispatch_allocations(
            revision,
            &mut c,
            "protocol varp",
            opcodes.varp_small,
            &mut protocol,
        ),
    ];
    assert!(c.ingame, "all warmed inbound fixtures must remain valid");
    counts
}

#[test]
fn warmed_inbound_decode_and_apply_allocation_gates_cover_each_revision() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let counts = assert_revision_inbound_allocation_gate(revision);
        let expected = match revision {
            ClientRevision::R274 | ClientRevision::R377 => [7, 0, 0, 0, 1, 0],
            ClientRevision::R289 => [14, 0, 0, 1, 1, 0],
        };
        assert_eq!(
            counts, expected,
            "{revision:?} warmed inbound allocations changed by event family"
        );
        eprintln!("{revision:?} inbound allocation baseline: {counts:?}");
    }
}

#[test]
fn warmed_legacy_welcome_components_allocate_nothing() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        welcome_draw_allocations(revision);
    }
}

fn welcome_draw_allocations(revision: ClientRevision) {
    let mut c = client(revision);
    let codes = [613, 650, 651, 652, 653, 654, 655];
    for code in codes {
        c.set_iface(
            code as usize,
            IfType {
                id: code,
                client_code: code,
                ..Default::default()
            },
        );
    }
    c.staffmodlevel = 1;
    let draw = |c: &mut Client, pass: usize| {
        let (login_days, recovery_days, messages, warning, mute) = [
            (0, 0, 0, 0, true),
            (1, 1, 1, 0, false),
            (27, 27, 12345, 0, true),
            (0, 200, 0, 0, false),
            (1, 201, 1, 1, false),
        ][pass % 5];
        c.last_login_ip = 0xff020304u32 as i32;
        c.days_since_login = login_days;
        c.days_since_recovery_change = recovery_days;
        c.last_login_message_count = messages;
        c.members_warning = warning;
        c.report_abuse_mute_option = mute;
        for code in codes {
            c.client_component(code);
        }
    };
    for dns in [Some("a-long-hostname-for-the-login.example".into()), None] {
        c.last_login_dns_display = dns;
        for pass in 0..5 {
            draw(&mut c, pass);
        }
        let count = allocations(|| {
            for pass in 0..1000 {
                draw(&mut c, pass);
            }
        });
        assert_eq!(
            count, 0,
            "{revision:?} warmed welcome draws must reuse text"
        );
        assert_eq!(c.if_(651).unwrap().text, "1 unread message");
    }
}

#[test]
fn warmed_report_name_draws_allocate_nothing() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let mut c = client(revision);
        c.set_iface(
            600,
            IfType {
                id: 600,
                client_code: 600,
                ..Default::default()
            },
        );
        c.report_abuse_input = "abcdefghijkl".into();
        c.client_component(600);
        let count = allocations(|| {
            for cycle in 0..1000 {
                c.loop_cycle = cycle;
                c.client_component(600);
            }
        });
        assert_eq!(count, 0, "{revision:?} warmed report draws must reuse text");
        assert_eq!(c.if_(600).unwrap().text, "abcdefghijkl ");
        c.loop_cycle = 0;
        c.client_component(600);
        assert_eq!(c.if_(600).unwrap().text, "abcdefghijkl|");
    }
}
