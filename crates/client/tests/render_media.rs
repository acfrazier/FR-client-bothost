// Task 6 (process-wide media): fonts + static media sprites are one
// process copy, so attaching a head is a pointer clone, not a re-depack.
// Two `Renderer::new_prefer` must hold the same `Arc<Media>` (ptr_eq) —
// both before any frame (the shared empty placeholder) and after the
// first frame depacked the real copy from the cache. This binary runs in
// its own process (each `tests/` file is a separate binary), so the
// process-wide `Media` OnceLock cannot race the other draw tests.
use client::render::Renderer;
use std::sync::Arc;

#[test]
fn renderers_share_one_media_arc() {
    let a = Renderer::new_prefer(false, false);
    let b = Renderer::new_prefer(false, false);
    assert!(
        Arc::ptr_eq(&a.media, &b.media),
        "two heads must share one fonts+media copy (a never-drawn renderer must not hold its own)"
    );
}

#[test]
fn renderers_that_drew_share_the_depacked_media_arc() {
    let mut a = Renderer::new_prefer(false, false);
    let mut b = Renderer::new_prefer(false, false);
    let mut c = client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.set_draw(true);
    // Both heads draw a title frame: the fonts/media depack runs once
    // (process-wide) and both renderers swap to the same Arc. The test
    // env has no `title`/`media` pack, so the sprites stay `None` — the
    // Arc itself is the assertion.
    a.title_screen_draw(&mut c);
    b.title_screen_draw(&mut c);
    assert!(
        Arc::ptr_eq(&a.media, &b.media),
        "two heads that drew a frame must share the depacked media copy"
    );
}
