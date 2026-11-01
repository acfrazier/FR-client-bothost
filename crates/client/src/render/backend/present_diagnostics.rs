//! Observation-only retained-resource events; absent without render-diagnostics.

use super::process_retained_present_resource_count;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static CLOCK: LazyLock<(Instant, u128)> = LazyLock::new(|| {
    (
        Instant::now(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    )
});
static ORDER: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static TEST_EVENTS: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, feature = "render-test-hooks"))]
pub(crate) fn begin_test_capture() {
    TEST_EVENTS.with_borrow_mut(|events| *events = Some(Vec::new()));
}

#[cfg(all(test, feature = "render-test-hooks"))]
pub(crate) fn take_test_events() -> Vec<String> {
    TEST_EVENTS.with_borrow_mut(|events| events.take().expect("capture was started"))
}

/// Last field of the retained owner: its drop follows the real permit release.
pub(super) struct Lifetime {
    pub(super) key: u64,
}

impl Drop for Lifetime {
    fn drop(&mut self) {
        emit("resource_released", self.key, None, None, None, None);
    }
}

struct Event {
    kind: &'static str,
    registration: u64,
    token: Option<u64>,
    producer_token: Option<u64>,
    held: Option<bool>,
    registered: Option<bool>,
    live_resources: usize,
    event_order: u64,
    elapsed_ns: u128,
    clock_origin_unix_ns: u128,
}

struct Nullable<T>(Option<T>);

impl<T: fmt::Display> fmt::Display for Nullable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(value) => value.fmt(f),
            None => f.write_str("null"),
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{\"schema\":\"render-present-lifetime-v1\",\"kind\":\"{}\",\"registration\":{},\"token\":{},\"producer_token\":{},\"held\":{},\"registered\":{},\"live_resources\":{},\"event_order\":{},\"elapsed_ns\":{},\"clock_origin_unix_ns\":{}}}",
            self.kind, self.registration, Nullable(self.token), Nullable(self.producer_token),
            Nullable(self.held), Nullable(self.registered), self.live_resources,
            self.event_order, self.elapsed_ns, self.clock_origin_unix_ns)
    }
}

pub(super) fn emit(
    kind: &'static str,
    registration: u64,
    token: Option<u64>,
    producer_token: Option<u64>,
    held: Option<bool>,
    registered: Option<bool>,
) {
    let clock = &*CLOCK;
    let event = Event {
        kind,
        registration,
        token,
        producer_token,
        held,
        registered,
        live_resources: process_retained_present_resource_count(),
        // Zero is an explicit unusable order if diagnostic identity exhausts.
        event_order: ORDER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .unwrap_or(0),
        elapsed_ns: clock.0.elapsed().as_nanos(),
        clock_origin_unix_ns: clock.1,
    };
    eprintln!("render_present_lifetime={event}");
    #[cfg(test)]
    TEST_EVENTS.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(event.to_string());
        }
    });
}

#[cfg(any(test, feature = "render-test-hooks"))]
pub(super) const fn static_storage_bytes() -> usize {
    std::mem::size_of::<LazyLock<(Instant, u128)>>() + std::mem::size_of::<AtomicU64>()
}

pub(super) fn released(state: &super::RetainedState) {
    emit(
        "released",
        state.key.0.get(),
        state.token.map(super::PresentToken::get),
        None,
        Some(state.held),
        Some(state.registered),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifetime_storage_is_fixed_and_only_contains_identity() {
        assert_eq!(std::mem::size_of::<Lifetime>(), std::mem::size_of::<u64>());
        assert_eq!(
            static_storage_bytes(),
            std::mem::size_of_val(&CLOCK) + std::mem::size_of_val(&ORDER)
        );
    }
}
