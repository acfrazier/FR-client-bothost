use client::client::Client;
use client::client::ClientConfig;
use client::graphics::Pix32;
use client::io::JagFile;

fn cache_dir() -> Option<String> {
    let cache = std::env::var("HOME").ok()? + "/experiments/Server/engine/data/pack/client";
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
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    assert_eq!(c.renderer.draw_area.width, 765);
    assert_eq!(c.renderer.draw_area.height, 503);
    c.title_screen_draw();
    assert!(c.renderer.draw_area.pixels.iter().any(|&p| p != 0));
}

/// `Client::new` must not start scape_main: the midi request is deferred
/// until the title screen is prepared (Client.ts maininit loads title/jags
/// first, then midiSong = 0 + onDemand.request(2, 0)).
#[test]
fn title_requests_scape_main() {
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
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    assert_eq!(c.midi_song, -1);
    c.title_screen_draw();
    assert_eq!(c.midi_song, -1);
}

/// title.dat JPEG is tiled into the left torch column (imageTitle0 at 0,0).
#[test]
fn title_background_fills_left_strip() {
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    c.title_screen_draw();
    let any = (0..265).any(|y| {
        (0..128).any(|x| c.renderer.draw_area.pixels[(y * c.renderer.draw_area.width + x) as usize] != 0)
    });
    assert!(any, "left title strip (torch / background) should not be black");
}

/// TitleFlames.renderFlames mutates imageTitle0 across ticks.
#[test]
fn title_flames_tick_mutates_left_strip() {
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    c.title_screen_draw();
    let before = c.renderer.image_title0.as_ref().expect("image_title0").pixels.clone();
    for _ in 0..8 {
        c.loop_cycle += 1;
        c.title_screen_draw();
    }
    let after = &c.renderer.image_title0.as_ref().expect("image_title0").pixels;
    assert_ne!(&before, after, "torch flame pixels should change across frames");
}

#[cfg(feature = "audio")]
#[test]
fn title_loads_engine_soundfont() {
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
    let Some(cache) = cache_dir() else {
        return;
    };
    let bytes = std::fs::read(format!("{cache}/title")).unwrap();
    let jag = JagFile::new(bytes);
    let img = Pix32::from_jpeg(&jag, "title.dat").expect("title.dat jpeg");
    assert!(img.wi > 0 && img.hi > 0);
    assert!(img.data.iter().any(|&p| p != 0));
}
