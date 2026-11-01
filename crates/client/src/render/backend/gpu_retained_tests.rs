use super::{context, GpuContext, PresentGate, PresentSubmit};
use crate::render::backend::{
    configure_retained_present_budget, CaptureGeometry, ConsumerFrame, PendingReadback,
    PresentToken, ReadbackBudget, ReadbackError, ReadbackKind, TextureHandle,
};
use std::num::NonZeroUsize;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const RETAINED_PRESENT_TEST_CAP: usize = 16;
const PIXEL: CaptureGeometry = CaptureGeometry::new(1, 1);

static GPU_TEST_SERIAL: Mutex<()> = Mutex::new(());
static RETAINED_PRESENT_BUDGET: LazyLock<()> = LazyLock::new(|| {
    configure_retained_present_budget(NonZeroUsize::new(RETAINED_PRESENT_TEST_CAP).unwrap())
        .expect("the retained-present test process configures its cap once");
});

fn fixture() -> (MutexGuard<'static, ()>, Arc<GpuContext>) {
    let serial = GPU_TEST_SERIAL
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    LazyLock::force(&RETAINED_PRESENT_BUDGET);
    let context = context().expect("the explicitly invoked retained proof requires a GPU adapter");
    (serial, context)
}

struct Producer {
    context: Arc<GpuContext>,
    gate: Arc<PresentGate>,
    source: wgpu::Texture,
    present: wgpu::Texture,
}

impl Producer {
    fn new(context: Arc<GpuContext>) -> Self {
        let texture = |label| {
            context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: PIXEL.width(),
                    height: PIXEL.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let source = texture("r274 retained acceptance source");
        let present = texture("r274 retained acceptance display");
        Self {
            context,
            gate: PresentGate::new(),
            source,
            present,
        }
    }

    fn submit(&self, rgba: [u8; 4], valid_scene: bool) -> Result<TextureHandle, PresentSubmit> {
        let source_view = self.source.create_view(&Default::default());
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("r274 retained acceptance paint"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("r274 retained acceptance source clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &source_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(rgba[0]) / 255.0,
                            g: f64::from(rgba[1]) / 255.0,
                            b: f64::from(rgba[2]) / 255.0,
                            a: f64::from(rgba[3]) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let token = match self.gate.submit_present(
            &self.context.queue,
            encoder,
            &self.source,
            &self.present,
            valid_scene,
        ) {
            PresentSubmit::Fresh(token) => token,
            other => return Err(other),
        };
        Ok(TextureHandle::presented(
            self.context.device.clone(),
            self.context.queue.clone(),
            self.present.create_view(&Default::default()),
            PIXEL,
            token,
            valid_scene,
            Arc::clone(&self.gate),
            true,
            None,
        ))
    }

    fn paint(&self, rgba: [u8; 4], valid_scene: bool) -> TextureHandle {
        self.submit(rgba, valid_scene)
            .unwrap_or_else(|result| panic!("present submission refused: {result:?}"))
    }

    fn copy_held(&self, frame: ConsumerFrame, kind: ReadbackKind) -> PendingReadback {
        let permit = ReadbackBudget::process()
            .try_acquire(PIXEL, Instant::now() + Duration::from_secs(30))
            .expect("the single process readback slot is available");
        match frame.try_begin_readback(kind, permit) {
            Ok(pending) => pending,
            Err((error, frame, permit)) => {
                drop(frame);
                drop(permit);
                panic!("held retained frame copy was refused: {error:?}");
            }
        }
    }
}

fn collect(mut pending: PendingReadback) -> (Vec<u8>, PresentToken, bool) {
    let timeout = Instant::now() + Duration::from_secs(10);
    loop {
        match pending.poll(Instant::now()) {
            Ok(Some(completed)) => {
                let (bytes, token, valid_scene, geometry, permit) = completed.into_parts();
                assert_eq!(geometry, PIXEL);
                drop(permit);
                return (bytes, token, valid_scene);
            }
            Ok(None) => {
                assert!(Instant::now() < timeout, "retained pixel copy timed out");
                std::thread::yield_now();
            }
            Err(error) => panic!("retained pixel copy failed: {error:?}"),
        }
    }
}

fn take_frame(registration: &crate::render::backend::ConsumerRegistration) -> ConsumerFrame {
    registration
        .try_take_frame()
        .expect("consumer admission stays live")
        .expect("a successful present offers one retained frame")
}

fn assert_pixel(bytes: &[u8], expected: [u8; 4]) {
    assert_eq!(
        bytes, expected,
        "the retained copy must preserve exact RGBA bytes"
    );
}

#[cfg(feature = "render-diagnostics")]
fn diagnostic_events() -> Vec<serde_json::Value> {
    let events: Vec<serde_json::Value> = super::present::diagnostics::take_test_events()
        .into_iter()
        .map(|row| serde_json::from_str(&row).expect("actual emitted event is valid JSON"))
        .collect();
    assert!(
        !events.is_empty(),
        "real lifecycle operations must emit observations"
    );
    let origin = events[0]["clock_origin_unix_ns"].as_u64().unwrap();
    assert!(origin > 0);
    for event in &events {
        assert_eq!(event["clock_origin_unix_ns"], origin);
        assert!(event["event_order"].as_u64().unwrap() > 0);
    }
    for pair in events.windows(2) {
        assert!(pair[0]["event_order"].as_u64() < pair[1]["event_order"].as_u64());
        assert!(pair[0]["elapsed_ns"].as_u64() <= pair[1]["elapsed_ns"].as_u64());
    }
    events
}

#[cfg(feature = "render-diagnostics")]
#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn diagnostics_keep_unregistered_held_resource_live_until_real_release() {
    let (_serial, context) = fixture();
    let baseline = super::present::process_retained_present_resource_count();
    super::present::diagnostics::begin_test_capture();
    let producer = Producer::new(context);
    let first = producer.paint([0, 0, 0, 255], true);
    let registration = first.try_register_consumer().unwrap();
    let key = registration.diagnostic_key();
    let next = producer.paint([255, 0, 0, 255], true);
    let held = take_frame(&registration);
    let token = held.present_token();
    drop(registration);
    producer.gate.retire();
    assert_eq!(
        super::present::process_retained_present_resource_count(),
        baseline + 1
    );
    drop(held);
    assert_eq!(
        super::present::process_retained_present_resource_count(),
        baseline
    );
    let events = diagnostic_events();
    let unregistered = events
        .iter()
        .find(|event| event["kind"] == "unregistered")
        .unwrap();
    assert_eq!(unregistered["registration"], key);
    assert_eq!(unregistered["token"], token.get());
    assert_eq!(unregistered["held"], true);
    assert_eq!(unregistered["registered"], false);
    assert_eq!(unregistered["live_resources"], baseline + 1);
    let retirement = events
        .iter()
        .position(|event| event["kind"] == "producer_retired")
        .unwrap();
    let release = events
        .iter()
        .position(|event| event["kind"] == "released")
        .unwrap();
    assert!(retirement < release);
    assert_eq!(events[release]["held"], false);
    assert_eq!(events[release]["registered"], false);
    assert_eq!(
        events[release]["token"],
        next.present_token().unwrap().get()
    );
    let final_event = events.last().unwrap();
    assert_eq!(final_event["kind"], "resource_released");
    assert_eq!(final_event["registration"], key);
    assert_eq!(final_event["live_resources"], baseline);
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn untracked_texture_cannot_register_a_scene_consumer() {
    let (_serial, context) = fixture();
    let before = crate::render::backend::present::process_retained_present_resource_count();
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("r274 untracked registration fixture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let handle = TextureHandle::untracked(
        context.device.clone(),
        context.queue.clone(),
        texture.create_view(&Default::default()),
        1,
        1,
    );
    assert_eq!(handle.present_token(), None);
    assert!(!handle.has_valid_scene());
    match handle.try_register_consumer() {
        Err(ReadbackError::Ungated) => {}
        Err(error) => panic!("untracked registration must fail as Ungated, got {error:?}"),
        Ok(_) => panic!("untracked texture unexpectedly registered"),
    }
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        before,
        "refused untracked registration allocates no retained resource"
    );
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn registered_delayed_consumer_keeps_n_across_next_paint() {
    let (_serial, context) = fixture();
    #[cfg(feature = "render-diagnostics")]
    let diagnostic_baseline = {
        super::present::diagnostics::begin_test_capture();
        super::present::process_retained_present_resource_count()
    };
    let producer = Producer::new(context);
    let first = producer.paint([0, 0, 0, 255], true);
    let registration = first
        .try_register_consumer()
        .expect("tracked producer registers one retained texture");
    #[cfg(feature = "render-diagnostics")]
    let diagnostic_key = registration.diagnostic_key();
    let frame_n = producer.paint([255, 0, 0, 255], true);
    let token_n = frame_n
        .present_token()
        .expect("submitted frame N has a token");
    let held_n = take_frame(&registration);
    assert_eq!(held_n.present_token(), token_n);
    assert!(held_n.has_valid_scene());

    let frame_n1 = producer.paint([0, 0, 255, 255], true);
    assert_ne!(frame_n1.present_token(), Some(token_n));
    assert_eq!(
        frame_n1.read_back(),
        [0x0000ff],
        "ordinary display publishes N+1"
    );
    assert!(
        registration
            .try_take_frame()
            .expect("registration remains valid")
            .is_none(),
        "a held N prevents a second retained offer"
    );

    let (bytes, token, valid) = collect(producer.copy_held(held_n, ReadbackKind::ValidScene));
    assert_pixel(&bytes, [255, 0, 0, 255]);
    assert_eq!(token, token_n);
    assert!(valid);
    drop(registration);
    assert_eq!(producer.gate.retained_present_bytes(), 0);
    #[cfg(feature = "render-diagnostics")]
    {
        let events = diagnostic_events();
        let held = events.iter().find(|event| event["kind"] == "held").unwrap();
        let later = events
            .iter()
            .find(|event| event["kind"] == "presented_while_held")
            .unwrap();
        assert_eq!(held["registration"], diagnostic_key);
        assert_eq!(later["registration"], diagnostic_key);
        assert_eq!(held["token"], token_n.get());
        assert_eq!(later["token"], token_n.get());
        assert_eq!(
            later["producer_token"],
            frame_n1.present_token().unwrap().get()
        );
        assert_eq!(held["held"], true);
        assert_eq!(held["registered"], true);
        assert_eq!(later["held"], true);
        assert_eq!(later["registered"], true);
        assert_eq!(held["live_resources"], diagnostic_baseline + 1);
        let released = events
            .iter()
            .find(|event| event["kind"] == "released")
            .unwrap();
        assert_eq!(released["token"], token_n.get());
        assert_eq!(released["held"], false);
        let final_event = events.last().unwrap();
        assert_eq!(final_event["kind"], "resource_released");
        assert_eq!(final_event["registration"], diagnostic_key);
        assert_eq!(final_event["live_resources"], diagnostic_baseline);
        assert!(final_event["held"].is_null());
        assert!(final_event["registered"].is_null());
    }
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn registered_delayed_consumer_survives_retire_without_aliasing_reattach() {
    let (_serial, context) = fixture();
    let baseline = crate::render::backend::present::process_retained_present_resource_count();
    let old = Producer::new(Arc::clone(&context));
    let old_first = old.paint([0, 0, 0, 255], true);
    let old_registration = old_first
        .try_register_consumer()
        .expect("old tracked head registers");
    let old_n = old.paint([255, 0, 0, 255], true);
    let old_token = old_n.present_token().expect("old frame token");
    let held_old = take_frame(&old_registration);
    old.gate.retire();
    assert!(matches!(
        old_registration.try_take_frame(),
        Err(ReadbackError::Retired)
    ));
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + 1,
        "retirement keeps the held old retained resource charged"
    );

    let new = Producer::new(Arc::clone(&context));
    let new_first = new.paint([0, 0, 0, 255], true);
    let new_registration = new_first
        .try_register_consumer()
        .expect("reattached head receives a distinct retained resource");
    let new_n = new.paint([0, 0, 255, 255], true);
    let new_token = new_n.present_token().expect("new frame token");
    assert_ne!(
        new_token, old_token,
        "reattach never aliases the retired token"
    );
    let held_new = take_frame(&new_registration);
    assert_eq!(held_new.present_token(), new_token);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + 2,
        "old draining and new active heads are charged independently"
    );

    let old_pending = old.copy_held(held_old, ReadbackKind::ValidScene);
    drop(old_registration);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + 1,
        "ordered submission releases the old retained texture, not the new one"
    );
    let (bytes, token, valid) = collect(old_pending);
    assert_pixel(&bytes, [255, 0, 0, 255]);
    assert_eq!(token, old_token);
    assert!(valid);

    let new_pending = new.copy_held(held_new, ReadbackKind::ValidScene);
    let (bytes, token, valid) = collect(new_pending);
    assert_pixel(&bytes, [0, 0, 255, 255]);
    assert_eq!(token, new_token);
    assert!(valid);
    drop(new_registration);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline
    );
    drop(old_first);
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn released_consumer_token_cannot_copy_reused_storage() {
    let (_serial, context) = fixture();
    let producer = Producer::new(context);
    let first = producer.paint([0, 0, 0, 255], true);
    let registration = first
        .try_register_consumer()
        .expect("tracked producer registers one retained texture");
    let frame_n = producer.paint([255, 0, 0, 255], true);
    let token_n = frame_n
        .present_token()
        .expect("submitted frame N has a token");
    let stale_n = take_frame(&registration);

    // Exercise the same one-shot release transition used by readback submission
    // while keeping the old capability value to test its rejection after reuse.
    stale_n.release_gate_resource_if_unregistered();
    let frame_n1 = producer.paint([0, 0, 255, 255], true);
    let token_n1 = frame_n1
        .present_token()
        .expect("submitted frame N+1 has a token");
    assert_ne!(token_n1, token_n);

    let permit = ReadbackBudget::process()
        .try_acquire(PIXEL, Instant::now() + Duration::from_secs(30))
        .expect("stale-token test owns the one readback permit");
    let (error, stale_n, permit) = match stale_n.try_begin_readback(ReadbackKind::AnyFrame, permit)
    {
        Err(rejected) => rejected,
        Ok(_) => panic!("released token unexpectedly copied reused storage"),
    };
    assert_eq!(error, ReadbackError::StalePresent);
    drop(stale_n);
    drop(permit);

    let held_n1 = take_frame(&registration);
    assert_eq!(held_n1.present_token(), token_n1);
    let (bytes, token, valid) = collect(producer.copy_held(held_n1, ReadbackKind::ValidScene));
    assert_pixel(&bytes, [0, 0, 255, 255]);
    assert_eq!(token, token_n1);
    assert!(valid);
    drop(registration);
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn ordered_copy_keeps_n_bytes_across_next_paint() {
    let (_serial, context) = fixture();
    let producer = Producer::new(context);
    let first = producer.paint([0, 0, 0, 255], true);
    let registration = first
        .try_register_consumer()
        .expect("tracked producer registers one retained texture");
    let frame_n = producer.paint([255, 0, 0, 255], true);
    let token_n = frame_n
        .present_token()
        .expect("submitted frame N has a token");
    let held_n = take_frame(&registration);
    assert_eq!(held_n.present_token(), token_n);

    // This submits retained-N -> staging before releasing N. The following
    // present reuses the retained texture, but the staging bytes are immutable.
    let pending_n = producer.copy_held(held_n, ReadbackKind::ValidScene);
    let frame_n1 = producer.paint([0, 0, 255, 255], true);
    let token_n1 = frame_n1
        .present_token()
        .expect("submitted frame N+1 has a token");
    assert_ne!(token_n1, token_n);
    assert_eq!(frame_n1.read_back(), [0x0000ff]);

    let (bytes, token, valid) = collect(pending_n);
    assert_pixel(&bytes, [255, 0, 0, 255]);
    assert_eq!(token, token_n);
    assert!(valid);
    drop(registration);
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn unregistered_head_never_allocates_or_copies_retained_present() {
    let (_serial, context) = fixture();
    let baseline = crate::render::backend::present::process_retained_present_resource_count();
    let producer = Producer::new(context);
    for color in [[0, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]] {
        let _ordinary = producer.paint(color, true);
        assert_eq!(producer.gate.retained_present_bytes(), 0);
        assert_eq!(
            crate::render::backend::present::process_retained_present_resource_count(),
            baseline,
            "ordinary paints without registration allocate no retained present"
        );
    }

    let registration_source = producer.paint([0, 0, 0, 255], true);
    let registration = registration_source
        .try_register_consumer()
        .expect("one explicit registration allocates one retained image");
    assert_eq!(producer.gate.retained_present_bytes(), 4);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + 1
    );

    let first_registered = producer.paint([255, 0, 0, 255], true);
    let first_token = first_registered.present_token().unwrap();
    let (bytes, token, valid) =
        collect(producer.copy_held(take_frame(&registration), ReadbackKind::ValidScene));
    assert_pixel(&bytes, [255, 0, 0, 255]);
    assert_eq!(token, first_token);
    assert!(valid);
    assert_eq!(producer.gate.retained_present_bytes(), 4);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + 1,
        "the same retained resource is reused after readback submission"
    );

    let second_registered = producer.paint([0, 0, 255, 255], true);
    let second_token = second_registered.present_token().unwrap();
    assert_ne!(second_token, first_token);
    let (bytes, token, valid) =
        collect(producer.copy_held(take_frame(&registration), ReadbackKind::ValidScene));
    assert_pixel(&bytes, [0, 0, 255, 255]);
    assert_eq!(token, second_token);
    assert!(valid);
    assert_eq!(producer.gate.retained_present_bytes(), 4);
    drop(registration);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline
    );
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn held_consumer_backpressure_never_overwrites_or_multiplies_resources() {
    let (_serial, context) = fixture();
    let baseline = crate::render::backend::present::process_retained_present_resource_count();
    let mut producers = Vec::with_capacity(RETAINED_PRESENT_TEST_CAP);
    let mut registrations = Vec::with_capacity(RETAINED_PRESENT_TEST_CAP);
    let mut held_frames = Vec::with_capacity(RETAINED_PRESENT_TEST_CAP);

    for index in 0..RETAINED_PRESENT_TEST_CAP {
        let producer = Producer::new(Arc::clone(&context));
        let registration_source = producer.paint([0, 0, 0, 255], true);
        let registration = registration_source
            .try_register_consumer()
            .expect("each head receives one retained permit up to q");
        let color = [index as u8, 0, 0, 255];
        producer.paint(color, true);
        let held = take_frame(&registration);
        assert_eq!(
            crate::render::backend::present::process_retained_present_resource_count(),
            baseline + index + 1
        );
        producers.push(producer);
        registrations.push(registration);
        held_frames.push(held);
    }
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + RETAINED_PRESENT_TEST_CAP,
        "all q retained resources are charged while held"
    );

    for (producer, registration, held) in producers
        .iter()
        .zip(&registrations)
        .zip(&held_frames)
        .map(|((producer, registration), held)| (producer, registration, held))
    {
        let next = producer.paint([0, 0, 255, 255], true);
        assert!(next.fresh, "held N does not block ordinary display N+1");
        assert_eq!(next.read_back(), [0x0000ff]);
        assert!(
            registration
                .try_take_frame()
                .expect("registration stays live")
                .is_none(),
            "held bytes are not overwritten with another offer"
        );
        assert_ne!(
            held.present_token(),
            next.present_token()
                .expect("submitted frame N+1 has a token"),
            "the held offer remains N while the ordinary display advances to N+1"
        );
    }

    producers[0].gate.retire();
    assert!(matches!(
        registrations[0].try_take_frame(),
        Err(ReadbackError::Retired)
    ));
    let replacement = Producer::new(Arc::clone(&context));
    let replacement_frame = replacement.paint([0, 0, 0, 255], true);
    match replacement_frame.try_register_consumer() {
        Err(ReadbackError::Limit) => {}
        Err(error) => panic!("full retained budget must refuse with Limit, got {error:?}"),
        Ok(_) => panic!("q+1 resource unexpectedly allocated"),
    }
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + RETAINED_PRESENT_TEST_CAP,
        "reattach cannot multiply the q active-or-draining resources"
    );

    let old_frame = held_frames.remove(0);
    let old_pending = producers[0].copy_held(old_frame, ReadbackKind::ValidScene);
    drop(registrations.remove(0));
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + RETAINED_PRESENT_TEST_CAP - 1,
        "retired held N remains charged until its ordered copy is submitted"
    );
    let (bytes, _, valid) = collect(old_pending);
    assert_pixel(&bytes, [0, 0, 0, 255]);
    assert!(valid);

    let replacement_registration = replacement_frame
        .try_register_consumer()
        .expect("one released old resource admits the waiting replacement");
    let replacement_n = replacement.paint([0, 255, 0, 255], true);
    let held_replacement = take_frame(&replacement_registration);
    assert_eq!(
        held_replacement.present_token(),
        replacement_n
            .present_token()
            .expect("replacement N has a submitted token")
    );
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline + RETAINED_PRESENT_TEST_CAP
    );

    drop(held_replacement);
    drop(replacement_registration);
    drop(held_frames);
    drop(registrations);
    assert_eq!(
        crate::render::backend::present::process_retained_present_resource_count(),
        baseline,
        "all explicit holders release their actual retained resources"
    );
}

#[test]
#[ignore = "requires an explicitly configured headless GPU adapter"]
fn timed_out_readback_cannot_accumulate_driver_buffers() {
    let (_serial, context) = fixture();
    let producer = Producer::new(context);
    let initial = producer.paint([0, 0, 0, 255], true);
    let registration = initial
        .try_register_consumer()
        .expect("tracked producer registers one retained texture");
    producer.paint([255, 0, 0, 255], true);
    let held = take_frame(&registration);

    let mut completion_hold =
        crate::render::backend::readback::test_hooks::hold_next_map_completion();
    let mut pending = producer.copy_held(held, ReadbackKind::ValidScene);
    let callback_deadline = Instant::now() + Duration::from_secs(10);
    while !completion_hold.is_held() {
        assert!(
            Instant::now() < callback_deadline,
            "the real GPU map callback did not reach the controlled completion point"
        );
        assert!(
            matches!(pending.poll(Instant::now()), Ok(None)),
            "the map result must remain unresolved while its callback is held"
        );
        std::thread::yield_now();
    }

    let past_deadline = Instant::now() + Duration::from_secs(31);
    assert!(matches!(
        pending.poll(past_deadline),
        Err(ReadbackError::Timeout)
    ));
    assert!(
        matches!(
            ReadbackBudget::process().try_acquire(PIXEL, Instant::now() + Duration::from_secs(10)),
            Err(ReadbackError::Busy)
        ),
        "the unresolved staging buffer and permit stay quarantined"
    );

    completion_hold.release();
    let reap_deadline = Instant::now() + Duration::from_secs(10);
    let permit = loop {
        match ReadbackBudget::process().try_acquire(PIXEL, Instant::now() + Duration::from_secs(10))
        {
            Ok(permit) => break permit,
            Err(ReadbackError::Busy) => {
                assert!(
                    Instant::now() < reap_deadline,
                    "completed callback was not reaped"
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("reaping the completed map failed: {error:?}"),
        }
    };
    drop(permit);
    drop(completion_hold);
    drop(registration);
}
