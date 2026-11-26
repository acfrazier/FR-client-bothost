use super::packet_377::VarpPacketForm;
use super::{Client, Skill};
use std::collections::VecDeque;

/// Maximum number of retained decoded events for one armed client session.
pub const MAX_DECODED_DOMAIN_EVENTS: usize = 4096;
const MAX_VARP_ID: i32 = u16::MAX as i32;

/// Client-side IDs selected for passive decoded observation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedDomainSelection {
    pub stat_skills: Vec<i32>,
    pub varps: Vec<i32>,
    pub inventory_components: Vec<i32>,
}

impl DecodedDomainSelection {
    fn is_empty(&self) -> bool {
        self.stat_skills.is_empty() && self.varps.is_empty() && self.inventory_components.is_empty()
    }
}

/// Why the bounded decoded observer refused an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedDomainFault {
    NotArmed,
    AlreadyArmed,
    NoActiveSession,
    InvalidCapacity,
    InvalidSelection,
    BufferOverflow,
    SessionChanged,
    CounterOverflow,
    NotSelected,
    /// A packet failed to apply after it began; its partial events are void.
    ApplyFailed,
}

/// Counts written to caller-owned storage and the observer's retained fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedDomainDrain {
    pub written: usize,
    pub remaining: usize,
    pub fault: Option<DecodedDomainFault>,
}

/// Current per-session frame receipts. This value is available without an armed observer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedDomainWatermark {
    pub client_session: u64,
    pub source_serial: u64,
    pub application_serial: u64,
}

/// One selected varp's latest in-session source value and application receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedDomainVarpWitness {
    pub client_session: u64,
    pub id: i32,
    pub value: i32,
    pub source_serial: u64,
    pub application_serial: u64,
}

/// The packet form that supplied a varp value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarpForm {
    Small,
    Large,
    Sync,
}

/// The applied inventory update form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryForm {
    Full,
    Partial,
    StopTransmit,
}

/// The source packet and applied frame identity for one decoded event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedDomainStamp {
    pub client_session: u64,
    /// Receipt that supplied this value. A sync event keeps the setter receipt.
    pub source_serial: u64,
    /// Frame whose apply operation produced this event.
    pub application_serial: u64,
    /// Event order within that applied frame.
    pub application_order: u32,
}

/// A selected stat, varp or inventory event after the client applied it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedDomainEvent {
    Stat {
        stamp: DecodedDomainStamp,
        skill: i32,
        current_level: i32,
        wire_xp: i32,
    },
    Varp {
        stamp: DecodedDomainStamp,
        id: i32,
        value: i32,
        form: VarpForm,
    },
    InventoryMarker {
        stamp: DecodedDomainStamp,
        component: i32,
        form: InventoryForm,
    },
    InventorySlot {
        stamp: DecodedDomainStamp,
        component: i32,
        slot: usize,
        item_id: Option<i32>,
        count: i32,
        form: InventoryForm,
    },
}

struct SelectedVarp {
    id: i32,
    witness: Option<DecodedDomainVarpWitness>,
}

pub(super) struct DecodedDomainObserver {
    client_session: u64,
    stat_skills: Vec<i32>,
    varps: Vec<SelectedVarp>,
    inventory_components: Vec<i32>,
    events: VecDeque<DecodedDomainEvent>,
    capacity: usize,
    fault: Option<DecodedDomainFault>,
    current_application_serial: u64,
    next_application_order: u32,
}

impl DecodedDomainObserver {
    fn new(client_session: u64, selection: DecodedDomainSelection, capacity: usize) -> Self {
        let DecodedDomainSelection {
            stat_skills,
            varps,
            inventory_components,
        } = selection;
        let varps: Vec<SelectedVarp> = varps
            .into_iter()
            .map(|id| SelectedVarp { id, witness: None })
            .collect();
        let events =
            if stat_skills.is_empty() && varps.is_empty() && inventory_components.is_empty() {
                VecDeque::new()
            } else {
                VecDeque::with_capacity(capacity)
            };
        Self {
            client_session,
            stat_skills,
            varps,
            inventory_components,
            events,
            capacity,
            fault: None,
            current_application_serial: 0,
            next_application_order: 0,
        }
    }

    fn is_selected_inventory(&self, component: i32) -> bool {
        self.inventory_components.binary_search(&component).is_ok()
    }

    fn latch_fault(&mut self, fault: DecodedDomainFault) {
        if self.fault.is_none() {
            self.fault = Some(fault);
            self.events.clear();
        }
    }

    fn check_session(&self, client_session: u64) -> Result<(), DecodedDomainFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if client_session != self.client_session {
            return Err(DecodedDomainFault::SessionChanged);
        }
        Ok(())
    }

    fn begin_packet(&mut self, client_session: u64, application_serial: u64) {
        if self.fault.is_some() {
            return;
        }
        if client_session != self.client_session {
            self.latch_fault(DecodedDomainFault::SessionChanged);
            return;
        }
        self.current_application_serial = application_serial;
        self.next_application_order = 0;
    }

    fn next_stamp(
        &mut self,
        client_session: u64,
        source_serial: u64,
        application_serial: u64,
    ) -> Option<DecodedDomainStamp> {
        if self.fault.is_some() {
            return None;
        }
        if client_session != self.client_session {
            self.latch_fault(DecodedDomainFault::SessionChanged);
            return None;
        }
        if application_serial != self.current_application_serial {
            self.latch_fault(DecodedDomainFault::CounterOverflow);
            return None;
        }
        if self.events.len() >= self.capacity {
            self.latch_fault(DecodedDomainFault::BufferOverflow);
            return None;
        }
        let application_order = self.next_application_order;
        let Some(next_order) = application_order.checked_add(1) else {
            self.latch_fault(DecodedDomainFault::CounterOverflow);
            return None;
        };
        self.next_application_order = next_order;
        Some(DecodedDomainStamp {
            client_session,
            source_serial,
            application_serial,
            application_order,
        })
    }

    fn record_stat(
        &mut self,
        client_session: u64,
        source_serial: u64,
        application_serial: u64,
        skill: i32,
        current_level: i32,
        wire_xp: i32,
    ) {
        if self.stat_skills.binary_search(&skill).is_err() {
            return;
        }
        let Some(stamp) = self.next_stamp(client_session, source_serial, application_serial) else {
            return;
        };
        self.events.push_back(DecodedDomainEvent::Stat {
            stamp,
            skill,
            current_level,
            wire_xp,
        });
    }

    fn record_varp_direct(
        &mut self,
        client_session: u64,
        source_serial: u64,
        application_serial: u64,
        id: i32,
        value: i32,
        form: VarpForm,
    ) {
        let Ok(index) = self.varps.binary_search_by_key(&id, |entry| entry.id) else {
            return;
        };
        let Some(stamp) = self.next_stamp(client_session, source_serial, application_serial) else {
            return;
        };
        self.events.push_back(DecodedDomainEvent::Varp {
            stamp,
            id,
            value,
            form,
        });
        self.varps[index].witness = Some(DecodedDomainVarpWitness {
            client_session,
            id,
            value,
            source_serial,
            application_serial,
        });
    }

    fn record_varp_sync(
        &mut self,
        client_session: u64,
        application_serial: u64,
        id: i32,
        value: i32,
    ) {
        let Ok(index) = self.varps.binary_search_by_key(&id, |entry| entry.id) else {
            return;
        };
        let Some(previous) = self.varps[index].witness else {
            return;
        };
        let Some(stamp) =
            self.next_stamp(client_session, previous.source_serial, application_serial)
        else {
            return;
        };
        self.events.push_back(DecodedDomainEvent::Varp {
            stamp,
            id,
            value,
            form: VarpForm::Sync,
        });
        self.varps[index].witness = Some(DecodedDomainVarpWitness {
            client_session,
            id,
            value,
            source_serial: previous.source_serial,
            application_serial,
        });
    }
    fn record_varp_sync_all(
        &mut self,
        client_session: u64,
        application_serial: u64,
        values: &[i32],
    ) {
        if self.fault.is_some() {
            return;
        }
        for index in 0..self.varps.len() {
            let id = self.varps[index].id;
            let Some(value) = usize::try_from(id)
                .ok()
                .and_then(|id| values.get(id))
                .copied()
            else {
                continue;
            };
            self.record_varp_sync(client_session, application_serial, id, value);
        }
    }

    fn record_inventory_marker(
        &mut self,
        client_session: u64,
        source_serial: u64,
        application_serial: u64,
        component: i32,
        form: InventoryForm,
    ) {
        if !self.is_selected_inventory(component) {
            return;
        }
        let Some(stamp) = self.next_stamp(client_session, source_serial, application_serial) else {
            return;
        };
        self.events.push_back(DecodedDomainEvent::InventoryMarker {
            stamp,
            component,
            form,
        });
    }

    fn record_inventory_slot(
        &mut self,
        client_session: u64,
        source_serial: u64,
        application_serial: u64,
        component: i32,
        (slot, object_plus_one, count): (usize, i32, i32),
        form: InventoryForm,
    ) {
        if !self.is_selected_inventory(component) {
            return;
        }
        let Some(stamp) = self.next_stamp(client_session, source_serial, application_serial) else {
            return;
        };
        self.events.push_back(DecodedDomainEvent::InventorySlot {
            stamp,
            component,
            slot,
            item_id: (object_plus_one > 0).then_some(object_plus_one - 1),
            count,
            form,
        });
    }

    fn varp_witness(
        &self,
        client_session: u64,
        id: i32,
    ) -> Result<Option<DecodedDomainVarpWitness>, DecodedDomainFault> {
        self.check_session(client_session)?;
        let Ok(index) = self.varps.binary_search_by_key(&id, |entry| entry.id) else {
            return Err(DecodedDomainFault::NotSelected);
        };
        Ok(self.varps[index].witness)
    }

    fn drain_into<T>(
        &mut self,
        client_session: u64,
        output: &mut [Option<T>],
        mut map: impl FnMut(DecodedDomainEvent) -> T,
    ) -> DecodedDomainDrain {
        for slot in output.iter_mut() {
            *slot = None;
        }
        if client_session != self.client_session {
            self.latch_fault(DecodedDomainFault::SessionChanged);
        }
        if let Some(fault) = self.fault {
            return DecodedDomainDrain {
                written: 0,
                remaining: 0,
                fault: Some(fault),
            };
        }
        let written = output.len().min(self.events.len());
        for slot in output.iter_mut().take(written) {
            let event = self.events.pop_front().expect("counted observer event");
            *slot = Some(map(event));
        }
        DecodedDomainDrain {
            written,
            remaining: self.events.len(),
            fault: None,
        }
    }
}

impl Client {
    /// Arm a bounded passive observer for selected decoded domains.
    pub fn arm_decoded_domain_observer(
        &mut self,
        mut selection: DecodedDomainSelection,
        capacity: usize,
    ) -> Result<(), DecodedDomainFault> {
        if self.decoded_domain_observer.is_some() {
            return Err(DecodedDomainFault::AlreadyArmed);
        }
        if !self.ingame || self.gens.session == 0 {
            return Err(DecodedDomainFault::NoActiveSession);
        }
        if capacity > MAX_DECODED_DOMAIN_EVENTS || (capacity == 0 && !selection.is_empty()) {
            return Err(DecodedDomainFault::InvalidCapacity);
        }
        selection.stat_skills.sort_unstable();
        selection.stat_skills.dedup();
        selection.varps.sort_unstable();
        selection.varps.dedup();
        selection.inventory_components.sort_unstable();
        selection.inventory_components.dedup();
        if selection
            .stat_skills
            .iter()
            .any(|id| *id < 0 || *id as usize >= Skill::count)
            || selection
                .varps
                .iter()
                .any(|id| *id < 0 || *id > MAX_VARP_ID)
            || selection
                .inventory_components
                .iter()
                .any(|id| *id < 0 || *id as usize >= self.ifaces_mut.len())
        {
            return Err(DecodedDomainFault::InvalidSelection);
        }
        self.decoded_domain_observer = Some(Box::new(DecodedDomainObserver::new(
            self.gens.session,
            selection,
            capacity,
        )));
        Ok(())
    }

    /// Current per-session packet receipts, even when no observer is armed.
    pub fn decoded_domain_watermark(&self) -> Result<DecodedDomainWatermark, DecodedDomainFault> {
        if self.decoded_domain_counter_overflowed {
            return Err(DecodedDomainFault::CounterOverflow);
        }
        Ok(DecodedDomainWatermark {
            client_session: self.gens.session,
            source_serial: self.decoded_domain_source_serial,
            application_serial: self.decoded_domain_application_serial,
        })
    }

    /// Current receipt watermark for the armed observer.
    pub fn decoded_domain_observer_watermark(
        &self,
    ) -> Result<DecodedDomainWatermark, DecodedDomainFault> {
        let observer = self
            .decoded_domain_observer
            .as_ref()
            .ok_or(DecodedDomainFault::NotArmed)?;
        observer.check_session(self.gens.session)?;
        self.decoded_domain_watermark()
    }

    /// The latest selected varp witness. `None` means no selected packet supplied a value.
    pub fn decoded_domain_varp_witness(
        &self,
        id: i32,
    ) -> Result<Option<DecodedDomainVarpWitness>, DecodedDomainFault> {
        self.decoded_domain_observer
            .as_ref()
            .ok_or(DecodedDomainFault::NotArmed)?
            .varp_witness(self.gens.session, id)
    }

    /// Drain decoded events into caller-owned storage.
    pub fn drain_decoded_domain_observer(
        &mut self,
        output: &mut [Option<DecodedDomainEvent>],
    ) -> DecodedDomainDrain {
        self.drain_decoded_domain_observer_into(output, |event| event)
    }

    /// Drain and map events directly into caller-owned storage without an intermediate buffer.
    pub fn drain_decoded_domain_observer_into<T>(
        &mut self,
        output: &mut [Option<T>],
        map: impl FnMut(DecodedDomainEvent) -> T,
    ) -> DecodedDomainDrain {
        let Some(observer) = self.decoded_domain_observer.as_mut() else {
            for slot in output.iter_mut() {
                *slot = None;
            }
            return DecodedDomainDrain {
                written: 0,
                remaining: 0,
                fault: Some(DecodedDomainFault::NotArmed),
            };
        };
        observer.drain_into(self.gens.session, output, map)
    }

    /// Cancel and release the armed event queue.
    pub fn cancel_decoded_domain_observer(&mut self) {
        self.decoded_domain_observer = None;
    }

    pub(super) fn begin_decoded_domain_dispatch(&mut self) {
        if self.decoded_domain_counter_overflowed {
            if let Some(observer) = self.decoded_domain_observer.as_mut() {
                observer.latch_fault(DecodedDomainFault::CounterOverflow);
            }
            return;
        }
        let Some(source_serial) = self.decoded_domain_source_serial.checked_add(1) else {
            self.decoded_domain_counter_overflowed = true;
            if let Some(observer) = self.decoded_domain_observer.as_mut() {
                observer.latch_fault(DecodedDomainFault::CounterOverflow);
            }
            return;
        };
        let Some(application_serial) = self.decoded_domain_application_serial.checked_add(1) else {
            self.decoded_domain_counter_overflowed = true;
            if let Some(observer) = self.decoded_domain_observer.as_mut() {
                observer.latch_fault(DecodedDomainFault::CounterOverflow);
            }
            return;
        };
        self.decoded_domain_source_serial = source_serial;
        self.decoded_domain_current_application_serial = application_serial;
        if let Some(observer) = self.decoded_domain_observer.as_mut() {
            observer.begin_packet(self.gens.session, application_serial);
        }
    }

    pub(super) fn finish_decoded_domain_dispatch(&mut self) {
        if !self.decoded_domain_counter_overflowed {
            self.decoded_domain_application_serial = self.decoded_domain_current_application_serial;
        }
    }

    /// A packet that began dispatch did not apply cleanly (a protocol error
    /// or a caught panic). Events it already recorded are not an applied
    /// state, so latch `ApplyFailed` (which drops queued events) and still
    /// consume the serial, so no later packet reuses its stamps.
    pub(super) fn fail_decoded_domain_dispatch(&mut self) {
        if let Some(observer) = self.decoded_domain_observer.as_mut() {
            observer.latch_fault(DecodedDomainFault::ApplyFailed);
        }
        self.finish_decoded_domain_dispatch();
    }

    pub(super) fn reset_decoded_domain_counters(&mut self) {
        self.decoded_domain_source_serial = 0;
        self.decoded_domain_application_serial = 0;
        self.decoded_domain_current_application_serial = 0;
        self.decoded_domain_counter_overflowed = false;
    }

    pub(super) fn observe_decoded_stat(&mut self, skill: i32, level: i32, wire_xp: i32) {
        if let Some(observer) = self.decoded_domain_observer.as_mut() {
            observer.record_stat(
                self.gens.session,
                self.decoded_domain_source_serial,
                self.decoded_domain_current_application_serial,
                skill,
                level,
                wire_xp,
            );
        }
    }

    pub(super) fn observe_decoded_varp_direct(
        &mut self,
        id: i32,
        value: i32,
        form: VarpPacketForm,
    ) {
        let form = match form {
            VarpPacketForm::Small => VarpForm::Small,
            VarpPacketForm::Large => VarpForm::Large,
        };
        if let Some(observer) = self.decoded_domain_observer.as_mut() {
            observer.record_varp_direct(
                self.gens.session,
                self.decoded_domain_source_serial,
                self.decoded_domain_current_application_serial,
                id,
                value,
                form,
            );
        }
    }

    pub(super) fn observe_decoded_varp_sync(&mut self) {
        let client_session = self.gens.session;
        let application_serial = self.decoded_domain_current_application_serial;
        if let Some(observer) = self.decoded_domain_observer.as_mut() {
            observer.record_varp_sync_all(client_session, application_serial, &self.var);
        }
    }

    pub(super) fn observe_decoded_inventory_full(&mut self, component: i32) {
        if self.decoded_domain_observer.is_none() {
            return;
        }
        let Some(index) = usize::try_from(component).ok() else {
            return;
        };
        let Some(inventory) = self.ifaces_mut.get(index).and_then(Option::as_ref) else {
            return;
        };
        let (Some(types), Some(numbers)) = (
            inventory.link_obj_type.as_deref(),
            inventory.link_obj_number.as_deref(),
        ) else {
            return;
        };
        if types.len() != numbers.len() {
            return;
        }
        let Some(observer) = self.decoded_domain_observer.as_mut() else {
            return;
        };
        if !observer.is_selected_inventory(component) {
            return;
        }
        let client_session = self.gens.session;
        let source_serial = self.decoded_domain_source_serial;
        let application_serial = self.decoded_domain_current_application_serial;
        observer.record_inventory_marker(
            client_session,
            source_serial,
            application_serial,
            component,
            InventoryForm::Full,
        );
        for (slot, (&object_plus_one, &count)) in types.iter().zip(numbers).enumerate() {
            observer.record_inventory_slot(
                client_session,
                source_serial,
                application_serial,
                component,
                (slot, object_plus_one, count),
                InventoryForm::Full,
            );
        }
    }

    pub(super) fn observe_decoded_inventory_partial(
        &mut self,
        component: i32,
        entries: impl IntoIterator<Item = (i32, i32, i32)>,
    ) {
        if self.decoded_domain_observer.is_none() {
            return;
        }
        let Some(index) = usize::try_from(component).ok() else {
            return;
        };
        let Some(inventory) = self.ifaces_mut.get(index).and_then(Option::as_ref) else {
            return;
        };
        let (Some(types), Some(numbers)) = (
            inventory.link_obj_type.as_deref(),
            inventory.link_obj_number.as_deref(),
        ) else {
            return;
        };
        if types.len() != numbers.len() {
            return;
        }
        let Some(observer) = self.decoded_domain_observer.as_mut() else {
            return;
        };
        if !observer.is_selected_inventory(component) {
            return;
        }
        let client_session = self.gens.session;
        let source_serial = self.decoded_domain_source_serial;
        let application_serial = self.decoded_domain_current_application_serial;
        observer.record_inventory_marker(
            client_session,
            source_serial,
            application_serial,
            component,
            InventoryForm::Partial,
        );
        for (slot, object_plus_one, count) in entries {
            if slot >= 0 && (slot as usize) < types.len() {
                observer.record_inventory_slot(
                    client_session,
                    source_serial,
                    application_serial,
                    component,
                    (slot as usize, object_plus_one, count),
                    InventoryForm::Partial,
                );
            }
        }
    }

    pub(super) fn observe_decoded_inventory_stop(&mut self, component: i32) {
        if self.decoded_domain_observer.is_none() {
            return;
        }
        let Some(index) = usize::try_from(component).ok() else {
            return;
        };
        let Some(inventory) = self.ifaces_mut.get(index).and_then(Option::as_ref) else {
            return;
        };
        if inventory.link_obj_type.is_none() {
            return;
        }
        let Some(observer) = self.decoded_domain_observer.as_mut() else {
            return;
        };
        observer.record_inventory_marker(
            self.gens.session,
            self.decoded_domain_source_serial,
            self.decoded_domain_current_application_serial,
            component,
            InventoryForm::StopTransmit,
        );
    }
}
