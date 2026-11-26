use super::*;
use std::sync::Arc;

use crate::config::{Cache, IfType, IfTypeMut};
use crate::io::{Packet, ServerProt};

fn test_client(revision: ClientRevision) -> (Client, crate::test_support::IsolatedEnvironment) {
    let (mut client, environment) = crate::client::test_support::isolated_client(
        ClientConfig {
            host: String::new(),
            port: 0,
            cache_dir: String::new(),
            members: true,
            lowmem: false,
        },
        revision,
    );
    client.ingame = true;
    client.gens.session = 41;
    (client, environment)
}

fn arm(client: &mut Client, selection: DecodedDomainSelection, capacity: usize) {
    client
        .arm_decoded_domain_observer(selection, capacity)
        .expect("observer arm");
}

fn send(client: &mut Client, opcode: i32, bytes: &[u8]) {
    let mut packet = Packet::new(bytes.to_vec());
    client.psize = i32::try_from(bytes.len()).expect("bounded test packet");
    client.handle_packet(opcode, &mut packet);
}

fn send_stat(client: &mut Client, revision: ClientRevision, skill: u8, xp: i32, level: u8) {
    let xp = xp.to_be_bytes();
    match revision {
        ClientRevision::R274 => send(
            client,
            ServerProt::UPDATE_STAT,
            &[skill, xp[0], xp[1], xp[2], xp[3], level],
        ),
        ClientRevision::R289 => send(
            client,
            ServerProt289::UPDATE_STAT,
            &[skill, xp[0], xp[1], xp[2], xp[3], level],
        ),
        ClientRevision::R377 => send(
            client,
            49,
            &[0u8.wrapping_sub(skill), level, xp[0], xp[1], xp[2], xp[3]],
        ),
    }
}

fn send_varp_small(client: &mut Client, revision: ClientRevision, id: u16, value: i8) {
    match revision {
        ClientRevision::R274 => send(
            client,
            ServerProt::VARP_SMALL,
            &id.to_be_bytes()
                .into_iter()
                .chain([value as u8])
                .collect::<Vec<_>>(),
        ),
        ClientRevision::R289 => send(
            client,
            ServerProt289::VARP_SMALL,
            &id.to_be_bytes()
                .into_iter()
                .chain([value as u8])
                .collect::<Vec<_>>(),
        ),
        ClientRevision::R377 => {
            let [hi, lo] = id.to_be_bytes();
            // R377 g2_alt2 decodes [hi, lo + 128]. g1b_alt3 decodes 128 - byte.
            send(
                client,
                182,
                &[hi, lo.wrapping_add(128), 128u8.wrapping_sub(value as u8)],
            );
        }
    }
}

fn drain_events(client: &mut Client) -> (DecodedDomainDrain, [Option<DecodedDomainEvent>; 16]) {
    let mut events = [None; 16];
    let result = client.drain_decoded_domain_observer(&mut events);
    (result, events)
}

fn inventory_client(
    revision: ClientRevision,
) -> (Client, crate::test_support::IsolatedEnvironment) {
    let (mut client, environment) = test_client(revision);
    let mut overlays = vec![None; 4];
    let inventory = IfTypeMut {
        link_obj_type: Some(vec![0; 3]),
        link_obj_number: Some(vec![0; 3]),
        ..IfTypeMut::default()
    };
    overlays[3] = Some(Arc::new(inventory));
    client.ifaces_mut = Arc::new(overlays);
    if revision == ClientRevision::R377 {
        let mut decoded = vec![None; 4];
        decoded[3] = Some(Box::new(IfType::default()));
        client.ifaces = Arc::new(decoded);
    }
    (client, environment)
}

#[test]
fn unarmed_watermark_advances_without_an_observer() {
    let (mut client, _environment) = test_client(ClientRevision::R274);
    let before = client.decoded_domain_watermark().unwrap();
    assert_eq!(before.client_session, 41);
    assert_eq!(before.source_serial, 0);
    assert_eq!(before.application_serial, 0);

    send_stat(&mut client, ClientRevision::R274, 5, 12_345, 43);

    let after = client.decoded_domain_watermark().unwrap();
    assert_eq!(after.client_session, 41);
    assert_eq!(after.source_serial, 1);
    assert_eq!(after.application_serial, 1);
    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 0);
    assert_eq!(drain.fault, Some(DecodedDomainFault::NotArmed));
    assert!(events.iter().all(Option::is_none));
}

#[test]
fn selected_stat_repeats_keep_packet_and_application_identity_on_all_profiles() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let (mut client, _environment) = test_client(revision);
        arm(
            &mut client,
            DecodedDomainSelection {
                stat_skills: vec![5],
                ..DecodedDomainSelection::default()
            },
            8,
        );
        send_stat(&mut client, revision, 5, 12_345, 43);
        send_stat(&mut client, revision, 5, 12_345, 43);

        let (drain, events) = drain_events(&mut client);
        assert_eq!(drain.written, 2, "revision {revision:?}");
        assert_eq!(drain.remaining, 0);
        for (index, expected_serial) in [1, 2].into_iter().enumerate() {
            let Some(DecodedDomainEvent::Stat {
                stamp,
                skill,
                current_level,
                wire_xp,
            }) = events[index]
            else {
                panic!("revision {revision:?} did not emit a stat event");
            };
            assert_eq!(skill, 5);
            assert_eq!(current_level, 43);
            assert_eq!(wire_xp, 12_345);
            assert_eq!(stamp.client_session, 41);
            assert_eq!(stamp.source_serial, expected_serial);
            assert_eq!(stamp.application_serial, expected_serial);
            assert_eq!(stamp.application_order, 0);
        }
    }
}

#[test]
fn selected_varp_repeats_and_sync_preserve_value_source_receipt() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        let (mut client, _environment) = test_client(revision);
        arm(
            &mut client,
            DecodedDomainSelection {
                varps: vec![7],
                ..DecodedDomainSelection::default()
            },
            8,
        );
        send_varp_small(&mut client, revision, 7, 0);
        send_varp_small(&mut client, revision, 7, 0);

        let (drain, events) = drain_events(&mut client);
        assert_eq!(drain.written, 2);
        for expected_serial in [1, 2] {
            assert!(events.iter().flatten().any(|event| matches!(
                event,
                DecodedDomainEvent::Varp { stamp, id: 7, value: 0, form: VarpForm::Small }
                    if stamp.source_serial == expected_serial
                        && stamp.application_serial == expected_serial
            )));
        }
        let zero = client.decoded_domain_varp_witness(7).unwrap().unwrap();
        assert_eq!(zero.value, 0);
        assert_eq!(zero.source_serial, 2);
        assert_eq!(zero.application_serial, 2);

        send_varp_small(&mut client, revision, 7, 8);
        client.var[7] = 0;
        let sync_opcode = match revision {
            ClientRevision::R274 => ServerProt::VARP_SYNC,
            ClientRevision::R289 => ServerProt289::VARP_SYNC,
            ClientRevision::R377 => unreachable!(),
        };
        send(&mut client, sync_opcode, &[]);

        let (drain, events) = drain_events(&mut client);
        assert_eq!(drain.written, 2);
        assert!(matches!(
            events[0],
            Some(DecodedDomainEvent::Varp { stamp, id: 7, value: 8, form: VarpForm::Small })
                if stamp.source_serial == 3 && stamp.application_serial == 3
        ));
        assert!(matches!(
            events[1],
            Some(DecodedDomainEvent::Varp { stamp, id: 7, value: 8, form: VarpForm::Sync })
                if stamp.source_serial == 3 && stamp.application_serial == 4
        ));
        let synced = client.decoded_domain_varp_witness(7).unwrap().unwrap();
        assert_eq!(synced.value, 8);
        assert_eq!(synced.source_serial, 3);
        assert_eq!(synced.application_serial, 4);
    }
}

#[test]
fn revision_377_varp_decoder_preserves_small_and_large_form() {
    let (mut client, _environment) = test_client(ClientRevision::R377);
    client.cache = Arc::new(Cache {
        varps: (0..8).map(|_| crate::config::VarpType::default()).collect(),
        ..Cache::default()
    });
    arm(
        &mut client,
        DecodedDomainSelection {
            varps: vec![7],
            ..DecodedDomainSelection::default()
        },
        8,
    );
    send(&mut client, 182, &[0, 135, 128]);
    // The packet puts g4_alt3 value 5 before g2_alt1 id 7.
    send(&mut client, 115, &[0, 0, 5, 0, 7, 0]);

    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 2);
    assert!(matches!(
        events[0],
        Some(DecodedDomainEvent::Varp {
            id: 7,
            value: 0,
            form: VarpForm::Small,
            ..
        })
    ));
    assert!(matches!(
        events[1],
        Some(DecodedDomainEvent::Varp {
            id: 7,
            value: 5,
            form: VarpForm::Large,
            ..
        })
    ));
}

#[test]
fn inventory_full_captures_zero_tail_and_partial_captures_only_applied_rows() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        let (mut client, _environment) = inventory_client(revision);
        arm(
            &mut client,
            DecodedDomainSelection {
                inventory_components: vec![3],
                ..DecodedDomainSelection::default()
            },
            16,
        );
        match revision {
            ClientRevision::R274 => send(
                &mut client,
                ServerProt::UPDATE_INV_FULL,
                &[0, 3, 1, 0, 101, 4],
            ),
            ClientRevision::R289 => send(
                &mut client,
                ServerProt289::UPDATE_INV_FULL,
                &[0, 3, 0, 1, 0, 101, 4],
            ),
            ClientRevision::R377 => unreachable!(),
        }
        let (drain, events) = drain_events(&mut client);
        assert_eq!(drain.written, 4);
        assert!(matches!(
            events[0],
            Some(DecodedDomainEvent::InventoryMarker {
                component: 3,
                form: InventoryForm::Full,
                ..
            })
        ));
        assert!(matches!(
            events[1],
            Some(DecodedDomainEvent::InventorySlot {
                component: 3,
                slot: 0,
                item_id: Some(100),
                count: 4,
                ..
            })
        ));
        assert!(matches!(
            events[2],
            Some(DecodedDomainEvent::InventorySlot {
                component: 3,
                slot: 1,
                item_id: None,
                count: 0,
                ..
            })
        ));
        assert!(matches!(
            events[3],
            Some(DecodedDomainEvent::InventorySlot {
                component: 3,
                slot: 2,
                item_id: None,
                count: 0,
                ..
            })
        ));

        let partial_opcode = match revision {
            ClientRevision::R274 => ServerProt::UPDATE_INV_PARTIAL,
            ClientRevision::R289 => ServerProt289::UPDATE_INV_PARTIAL,
            ClientRevision::R377 => unreachable!(),
        };
        send(&mut client, partial_opcode, &[0, 3, 1, 0, 102, 9]);
        let (drain, events) = drain_events(&mut client);
        assert_eq!(drain.written, 2);
        assert!(matches!(
            events[0],
            Some(DecodedDomainEvent::InventoryMarker {
                form: InventoryForm::Partial,
                ..
            })
        ));
        assert!(matches!(
            events[1],
            Some(DecodedDomainEvent::InventorySlot {
                slot: 1,
                item_id: Some(101),
                count: 9,
                form: InventoryForm::Partial,
                ..
            })
        ));
    }
}

#[test]
fn revision_377_inventory_emits_full_partial_and_stop_markers() {
    let (mut client, _environment) = inventory_client(ClientRevision::R377);
    arm(
        &mut client,
        DecodedDomainSelection {
            inventory_components: vec![3],
            ..DecodedDomainSelection::default()
        },
        16,
    );
    // Full packet with no rows clears all three slots.
    send(&mut client, 206, &[0, 3, 0, 0]);
    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 4);
    assert!(matches!(
        events[0],
        Some(DecodedDomainEvent::InventoryMarker {
            form: InventoryForm::Full,
            ..
        })
    ));
    assert!(events[1..4].iter().all(|event| matches!(
        event,
        Some(DecodedDomainEvent::InventorySlot {
            item_id: None,
            count: 0,
            ..
        })
    )));

    // Partial slot 1 is an explicit empty item. Out-of-range rows are ignored.
    send(&mut client, 134, &[0, 3, 1, 0, 0, 0]);
    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 2);
    assert!(matches!(
        events[0],
        Some(DecodedDomainEvent::InventoryMarker {
            form: InventoryForm::Partial,
            ..
        })
    ));
    assert!(matches!(
        events[1],
        Some(DecodedDomainEvent::InventorySlot {
            slot: 1,
            item_id: None,
            count: 0,
            form: InventoryForm::Partial,
            ..
        })
    ));

    // g2_alt1 encodes component 3 as little-endian [3, 0].
    send(&mut client, 219, &[3, 0]);
    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 1);
    assert!(matches!(
        events[0],
        Some(DecodedDomainEvent::InventoryMarker {
            component: 3,
            form: InventoryForm::StopTransmit,
            ..
        })
    ));
}

#[test]
fn overflow_is_sticky_and_clears_partial_history() {
    let (mut client, _environment) = test_client(ClientRevision::R274);
    arm(
        &mut client,
        DecodedDomainSelection {
            stat_skills: vec![0],
            ..DecodedDomainSelection::default()
        },
        1,
    );
    send_stat(&mut client, ClientRevision::R274, 0, 100, 1);
    send_stat(&mut client, ClientRevision::R274, 0, 200, 2);

    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 0);
    assert_eq!(drain.remaining, 0);
    assert_eq!(drain.fault, Some(DecodedDomainFault::BufferOverflow));
    assert!(events.iter().all(Option::is_none));
    assert_eq!(client.stat_xp[0], 200);
    assert_eq!(client.decoded_domain_watermark().unwrap().source_serial, 2);
    assert_eq!(
        client.decoded_domain_observer_watermark(),
        Err(DecodedDomainFault::BufferOverflow)
    );
}

#[test]
fn session_change_faults_the_observer_and_empty_selection_is_valid() {
    let (mut client, _environment) = test_client(ClientRevision::R274);
    arm(&mut client, DecodedDomainSelection::default(), 0);
    assert_eq!(
        client.decoded_domain_watermark().unwrap().client_session,
        41
    );
    client.gens.session += 1;
    let (drain, events) = drain_events(&mut client);
    assert_eq!(drain.written, 0);
    assert_eq!(drain.remaining, 0);
    assert_eq!(drain.fault, Some(DecodedDomainFault::SessionChanged));
    assert!(events.iter().all(Option::is_none));
}

#[test]
fn invalid_selection_and_unselected_varp_fail_closed() {
    let (mut client, _environment) = test_client(ClientRevision::R274);
    assert_eq!(
        client.arm_decoded_domain_observer(
            DecodedDomainSelection {
                stat_skills: vec![Skill::count as i32],
                ..DecodedDomainSelection::default()
            },
            4,
        ),
        Err(DecodedDomainFault::InvalidSelection)
    );
    assert_eq!(
        client.decoded_domain_varp_witness(7),
        Err(DecodedDomainFault::NotArmed)
    );
}
