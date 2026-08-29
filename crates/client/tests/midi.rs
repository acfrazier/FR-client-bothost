use client::sound::{Midi, NullMidi};

#[test]
fn null_midi_swallows_play() {
    let mut m = NullMidi;
    m.play(&[0, 1, 2], 0, true);
    m.set_volume(-400);
    m.stop();
}

fn client() -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn clientcode3_volume_ladder() {
    let mut c = client();
    c.apply_clientcode(3, 0);
    assert!(c.midi_active);
    assert_eq!(c.midi_volume, 0);
    c.apply_clientcode(3, 1);
    assert_eq!(c.midi_volume, -400);
    c.apply_clientcode(3, 2);
    assert_eq!(c.midi_volume, -800);
    c.apply_clientcode(3, 3);
    assert_eq!(c.midi_volume, -1200);
    c.apply_clientcode(3, 4);
    assert!(!c.midi_active);
}

#[test]
fn clientcode3_mute_stops_and_unmute_requests_next_song() {
    let mut c = client();
    c.next_midi_song = 42;

    // mute: stopMidi clears the fade flag
    c.apply_clientcode(3, 4);
    assert!(!c.midi_active);
    assert!(!c.midi_fading);

    // unmute: re-request nextMidiSong like Java/TS (onDemand is None here,
    // but the midiSong/fading state must be re-armed)
    c.apply_clientcode(3, 1);
    assert!(c.midi_active);
    assert_eq!(c.midi_song, 42);
    assert!(c.midi_fading);
    assert_eq!(c.midi_volume, -400);

    // same-value clientcode does not re-trigger (midiSong untouched)
    c.midi_song = 7;
    c.apply_clientcode(3, 1);
    assert_eq!(c.midi_song, 7);
}

#[test]
fn save_and_stop_midi_feed_the_backend() {
    let mut c = client();
    c.save_midi(&[0, 1, 2], true);
    assert!(c.midi_fading);
    c.stop_midi();
    assert!(!c.midi_fading);
    // midi_volume rides along with saveMidi like signlink midivol
    c.midi_volume = -800;
    c.save_midi(&[3, 4], false);
    assert_eq!(c.midi_volume, -800);
}

#[test]
fn logout_stops_midi_and_resets_song_state() {
    let mut c = client();
    c.midi_song = 5;
    c.next_midi_song = 6;
    c.next_music_delay = 100;
    c.midi_fading = true;
    c.logout();
    assert!(!c.midi_fading);
    assert_eq!(c.midi_song, -1);
    assert_eq!(c.next_midi_song, -1);
    assert_eq!(c.next_music_delay, 0);
}

/// Java `gameLoop` calls `soundsDoQueue` (9461 / TS 2193). Without that
/// call SYNTH_SOUND queues forever and teleports are silent.
/// `--cache` / maininit must unpack `sounds.dat` into JagFX (TS 1168-1171).
#[test]
fn client_new_unpacks_sounds_jag() {
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&cache).join("sounds").is_file() {
        return;
    }
    let c = client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    assert!(
        c.jagfx.synth.iter().any(|s| s.is_some()),
        "sounds.dat must populate JagFX"
    );
}

#[test]
fn game_loop_drains_synth_sound_queue() {
    let mut c = client();
    c.ingame = true;
    c.wave_ids[0] = 1;
    c.wave_loops[0] = 1;
    c.wave_delay[0] = 0;
    c.wave_count = 1;
    c.game_loop();
    assert_eq!(c.wave_count, 0);
}

#[test]
fn synth_sound_queue_drains_through_jagfx() {
    let mut c = client();
    c.wave_ids[0] = 1;
    c.wave_loops[0] = 1;
    c.wave_delay[0] = 0;
    c.wave_count = 1;
    c.sounds_do_queue();
    assert_eq!(c.wave_count, 0);
}

#[test]
fn synth_sound_queue_pushes_pcm_not_drops() {
    let mut c = client();
    let mut p = client::io::Packet::new(JAGFX_FIXTURE.to_vec());
    c.jagfx.init(&mut p);
    c.wave_ids[0] = 882;
    c.wave_loops[0] = 1;
    c.wave_delay[0] = 0;
    c.wave_count = 1;
    c.sounds_do_queue();
    assert_eq!(c.wave_count, 0);
    let queue = c.waves.lock().unwrap();
    // generate() leaves pos at 44 (header) + 771 PCM bytes
    assert_eq!(queue.len(), 771);
    assert!(queue.iter().any(|&s| s != 0), "fixture sound must be audible");
}

#[test]
fn jagfx_init_and_generate_from_engine_sounds() {
    let path = client::engine_dir().display().to_string();
    let bytes = std::fs::read(format!("{path}/data/pack/client/sounds"));
    let Ok(bytes) = bytes else { return; };
    let jag = client::io::JagFile::new(bytes);
    let Some(sounds_dat) = jag.read("sounds.dat") else { return; };

    let mut fx = client::sound::JagFX::default();
    let mut p = client::io::Packet::new(sounds_dat);
    fx.init(&mut p);

    let first_id = fx.synth.iter().position(|s| s.is_some()).unwrap_or(0) as i32;
    let wave = fx.generate(first_id, 1).expect("first sound in table");
    assert_eq!(&wave.data()[..4], b"RIFF");
    assert_eq!(&wave.data()[8..12], b"WAVE");
    assert_eq!(&wave.data()[36..40], b"data");
    // WAV data is 8-bit PCM at 22050 Hz (LE int32 at offset 24)
    assert_eq!(wave.data()[22], 1);
    assert_eq!(u32::from_le_bytes(wave.data()[24..28].try_into().unwrap()), 22050);
    assert!(wave.pos > 44);

    // looped generation must not panic and stays a valid header
    let wave2 = fx.generate(first_id, 3).expect("loop-capable sound");
    assert_eq!(&wave2.data()[..4], b"RIFF");
}

/// A `sounds.dat` slice with one sound (id 882) plus the 65535 terminator,
/// cut from the 274 engine pack. Decoded output was cross-checked byte-for-
/// byte against the client-ts `JagFX.ts`/`Tone.ts` reference (with a seeded
/// `Math.random` noise table), so this pins the whole synth chain hermetically.
const JAGFX_FIXTURE: &[u8] = &[
    0x03, 0x72, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x17, 0x70, 0x04, 0x00, 0x00, 0xee,
    0x98, 0x5a, 0x79, 0xb6, 0x46, 0xbf, 0x79, 0xab, 0x03, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x64, 0x05, 0x00, 0x00, 0xef, 0x9e, 0x47, 0x89, 0xc1, 0x8a, 0x6a,
    0x9b, 0x54, 0xfe, 0xd3, 0xd0, 0x02, 0x0d, 0xff, 0xff, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x64, 0x05, 0x00, 0x00, 0x80, 0x00, 0x3f, 0xff, 0x80, 0x00, 0x7f, 0xfe,
    0x80, 0x00, 0xbf, 0xfd, 0x80, 0x00, 0xff, 0xff, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x64, 0x05, 0x00, 0x00, 0x80, 0x00, 0x3f, 0xff, 0x80, 0x00, 0x7f, 0xfe, 0x80, 0x00,
    0xbf, 0xfd, 0x80, 0x00, 0xff, 0xff, 0x80, 0x00, 0x00, 0x80, 0x96, 0xc0, 0x78, 0x00, 0x64, 0x40,
    0x00, 0x64, 0xc0, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff,
];

/// FNV-1a 64: golden checksum of the generated WAV bytes.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[test]
fn jagfx_fixture_matches_ts_reference() {
    let mut fx = client::sound::JagFX::default();
    let mut p = client::io::Packet::new(JAGFX_FIXTURE.to_vec());
    fx.init(&mut p);
    assert!(fx.synth[882].is_some());

    for loops in [1, 3] {
        let wave = fx.generate(882, loops).expect("sound 882 in fixture");
        assert_eq!(wave.pos, 815);
        // golden from client-ts (seeded noise table), byte-identical
        assert_eq!(fnv1a64(&wave.data()[..wave.pos]), 0x1750_1084_4d34_2967);
    }
}


#[cfg(feature = "audio")]
mod rusty {
    use client::sound::{Midi, RustyMidi};

    #[test]
    fn no_font_stays_silent() {
        let mut m = RustyMidi::new("/nonexistent");
        // truncated midi: parse fails inside play and stays silent
        m.play(&[0x4d, 0x54, 0x68, 0x64], -400, true);
        m.set_volume(-1200); // documented no-op; gain lives on the Fade
        m.stop();
        m.render(&mut [0f32; 64], &mut [0f32; 64]);
    }

    /// Clip guard for the scape_main symptom: render the real title song
    /// through the engine soundfont at full volume (fade gain 1.0) and
    /// assert no sample reaches the i16 rail (`max(|i16|) < 32767`). Skips
    /// silently when the engine assets are absent, like
    /// `jagfx_init_and_generate_from_engine_sounds`. rustysynth's
    /// reverb/chorus are disabled in the backend (274 has no effects
    /// buses), so this pins the whole chain: a regression that pushes the
    /// mix toward the rail (e.g. re-enabling effects) is caught.
    #[test]
    fn scape_main_render_stays_below_i16_rail() {
        let engine = client::engine_dir();
        let font = engine.join("public/client/SCC1_Florestan.sf2");
        let song = client::content_dir().join("songs/scape main.mid");
        let mut m = match RustyMidi::with_sound_font(&font.display().to_string()) {
            Some(m) => m,
            None => return, // no engine soundfont → nothing to render
        };
        let Ok(song) = std::fs::read(&song) else { return };
        m.play(&song, 0, true);
        let mut left = vec![0f32; 22050 * 10];
        let mut right = vec![0f32; 22050 * 10];
        m.render(&mut left, &mut right);
        let peak = left
            .iter()
            .chain(right.iter())
            .fold(0f32, |p, s| p.max(s.abs()));
        assert!(peak > 0.01, "scape_main must render audibly");
        let rail = (peak * 32767.0) as i32;
        assert!(rail < 32767, "scape_main hard-clips: peak {rail}/32767");
    }
}
