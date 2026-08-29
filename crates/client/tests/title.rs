use client::client::Client;
use client::render::Renderer;
use client::client::ClientConfig;
use client::graphics::Pix32;
use client::io::JagFile;

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
    let any = (0..265).any(|y| {
        (0..128).any(|x| r.draw_area.pixels[(y * r.draw_area.width + x) as usize] != 0)
    });
    assert!(any, "left title strip (torch / background) should not be black");
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
    let before = r.image_title0.as_ref().expect("image_title0").pixels.clone();
    for _ in 0..8 {
        c.loop_cycle += 1;
        r.title_screen_draw(&mut c);
    }
    let after = &r.image_title0.as_ref().expect("image_title0").pixels;
    assert_ne!(&before, after, "torch flame pixels should change across frames");
}

/// GPU title chrome still ticks the torch columns into `draw_area` (and
/// the composited frame, when wgpu is up). `SKIP_GPU=1` / no adapter
/// falls back to CPU — then this is the same as the CPU flame test.
#[test]
fn gpu_title_flames_tick_left_strip() {
    if std::env::var("SKIP_GPU").ok().as_deref() == Some("1") {
        return;
    }
    let mut r = Renderer::new_prefer(false, true);
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    r.title_screen_draw(&mut c);
    let any = (0..265).any(|y| {
        (0..128).any(|x| r.draw_area.pixels[(y * r.draw_area.width + x) as usize] != 0)
    });
    assert!(any, "GPU title left torch column must not be black");
    let before = r.image_title0.as_ref().expect("image_title0").pixels.clone();
    for _ in 0..8 {
        c.loop_cycle += 1;
        r.title_screen_draw(&mut c);
    }
    let after = &r.image_title0.as_ref().expect("image_title0").pixels;
    assert_ne!(
        &before, after,
        "GPU title torch flame pixels should change across frames"
    );
}

#[cfg(feature = "audio")]
#[test]
fn title_loads_engine_soundfont() {
let mut r = Renderer::new(false);
    let Some(cache) = cache_dir() else {
        return;
    };
    let midi = client::sound::RustyMidi::new(&cache);
    assert!(
        midi.has_sound_font(),
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
