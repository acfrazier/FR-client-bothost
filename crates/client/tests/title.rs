use client::client::Client;
use client::client::ClientConfig;
use client::graphics::Pix32;
use client::io::JagFile;
use client::render::backend::{BackendKind, FrameOutput};
use client::render::Renderer;

fn cache_dir() -> Option<String> {
    let cache = client::cache_dir().display().to_string();
    if std::path::Path::new(&cache).join("title").is_file() {
        Some(cache)
    } else {
        None
    }
}

fn client(cache: String) -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    })
}

fn frame_pixels(frame: FrameOutput) -> Vec<i32> {
    match frame {
        FrameOutput::PixMap(map) => map.pixels,
        FrameOutput::Texture(texture) => texture.read_back(),
    }
}

fn flame_columns_changed(before: &[i32], after: &[i32]) -> bool {
    let width = 765usize;
    (0..265usize).any(|y| {
        (0..128usize)
            .chain(637..765usize)
            .any(|x| before[y * width + x] != after[y * width + x])
    })
}

#[test]
fn title_draw_writes_pixels() {
    let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    assert_eq!(r.draw_area.width, 765);
    assert_eq!(r.draw_area.height, 503);
    r.title_screen_draw(&mut c);
    assert!(r.draw_area.pixels.iter().any(|&p| p != 0));
}

/// `Client::new` must not start scape_main: the midi request is deferred
/// until the title screen is prepared (Client.ts maininit loads title/jags
/// first, then midiSong = 0 + onDemand.request(2, 0)).
#[test]
fn title_requests_scape_main() {
    let _r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let c = client(cache);
    assert_eq!(c.midi_song, -1);
}

/// Java `prepareTitle` does not request scape_main. Song 0 is requested
/// from `maininit` after OnDemand.init (`Client.java` 5164-5182). A
/// title draw before that must leave `midi_song == -1`.
#[test]
fn prepare_title_does_not_request_scape_main() {
    let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    assert_eq!(c.midi_song, -1);
    r.title_screen_draw(&mut c);
    assert_eq!(c.midi_song, -1);
}

/// title.dat JPEG is tiled into the left torch column (imageTitle0 at 0,0).
#[test]
fn title_background_fills_left_strip() {
    let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    r.title_screen_draw(&mut c);
    let any = (0..265)
        .any(|y| (0..128).any(|x| r.draw_area.pixels[(y * r.draw_area.width + x) as usize] != 0));
    assert!(
        any,
        "left title strip (torch / background) should not be black"
    );
}

/// TitleFlames.renderFlames mutates imageTitle0 across ticks.
#[test]
fn title_flames_tick_mutates_left_strip() {
    let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    r.title_screen_draw(&mut c);
    let before = r
        .image_title0
        .as_ref()
        .expect("image_title0")
        .pixels
        .clone();
    std::thread::sleep(std::time::Duration::from_millis(120));
    r.title_screen_draw(&mut c);
    let after = &r.image_title0.as_ref().expect("image_title0").pixels;
    assert_ne!(
        &before, after,
        "torch flame pixels should change across frames"
    );
}

/// GPU title chrome uploads the changing torch columns into the presented
/// texture at first boot and after the game tears down the first title.
#[test]
fn gpu_title_flames_animate_first_boot_and_after_logout() {
    if std::env::var("SKIP_GPU").ok().as_deref() == Some("1") {
        return;
    }
    let mut r = Renderer::new_prefer(false, true);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; GPU title flame test skips");
        return;
    }
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);

    let first_boot = frame_pixels(r.title_screen_draw(&mut c));
    std::thread::sleep(std::time::Duration::from_millis(120));
    let animated_boot = frame_pixels(r.title_screen_draw(&mut c));
    assert!(
        flame_columns_changed(&first_boot, &animated_boot),
        "GPU title torch pixels must change at first boot"
    );

    c.set_draw(true);
    c.ingame = true;
    let _ = r.mainredraw(&mut c);
    assert!(
        r.title_flames.is_none(),
        "entering the game must unload GPU title flames"
    );

    c.logout();
    let after_logout = frame_pixels(r.mainredraw(&mut c));
    std::thread::sleep(std::time::Duration::from_millis(120));
    let animated_logout = frame_pixels(r.mainredraw(&mut c));
    assert!(
        flame_columns_changed(&after_logout, &animated_logout),
        "GPU title torch pixels must change after logout"
    );
}

/// A sparse title paint must catch the flame simulation up at the TS 35 ms
/// cadence after the game tears the first title instance down.
#[test]
fn title_flames_keep_ts_rate_after_logout() {
    let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);

    r.title_screen_draw(&mut c);
    c.set_draw(true);
    c.ingame = true;
    let _ = r.mainredraw(&mut c);
    assert!(
        r.title_flames.is_none(),
        "entering the game must unload the first title flame instance"
    );

    c.logout();
    let _ = r.mainredraw(&mut c);
    let after_logout = r.title_flames.as_ref().expect("logout title flames").cycle;
    let logout_pixels = r.draw_area.pixels.clone();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let _ = r.mainredraw(&mut c);
    let after_sparse_paint = r.title_flames.as_ref().expect("logout title flames").cycle;

    assert!(
        after_sparse_paint - after_logout >= 3,
        "120 ms between paints must advance at least three 35 ms flame frames"
    );
    assert!(
        flame_columns_changed(&logout_pixels, &r.draw_area.pixels),
        "CPU title torch pixels must change after logout"
    );
}

#[cfg(feature = "audio")]
#[test]
fn title_loads_engine_soundfont() {
    let _r = Renderer::new(false);
    let Some(_cache) = cache_dir() else {
        return;
    };
    let midi = client::sound::RustyMidi::with_sound_font(
        &client::engine_dir()
            .join("public/client/SCC1_Florestan.sf2")
            .display()
            .to_string(),
    );
    assert!(
        midi.is_some_and(|m| m.has_sound_font()),
        "SCC1_Florestan.sf2 should load from engine/public"
    );
}

#[test]
fn from_jpeg_decodes_title_dat() {
    let _r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let bytes = std::fs::read(format!("{cache}/title")).unwrap();
    let jag = JagFile::new(bytes);
    let img = Pix32::from_jpeg(&jag, "title.dat").expect("title.dat jpeg");
    assert!(img.wi > 0 && img.hi > 0);
    assert!(img.data.iter().any(|&p| p != 0));
}
