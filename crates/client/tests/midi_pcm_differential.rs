#![cfg(all(feature = "audio", feature = "test-support"))]

//! Corpus-driven raw PCM evidence for the three RustySynth versions in
//! DEPS-BATCH-025.
//!
//! Run from `vendor/fr-client-rust` (the test intentionally fails if any
//! required environment variable or corpus asset is absent):
//!
//! ```text
//! DEPS_BATCH_025_PCM_CORPUS=/Volumes/dev-scratch/274bot-evidence/DEPS-BATCH-025/impl/corpus/audio/289 \
//! DEPS_BATCH_025_PCM_CAPTURE_DIR=/Volumes/dev-scratch/274bot-evidence/DEPS-BATCH-025/impl/pcm/1.3.6 \
//! DEPS_BATCH_025_PCM_FIXTURE_DIR=/Volumes/dev-scratch/274bot-evidence/DEPS-BATCH-025/impl/pcm-fixtures \
//! cargo test -p client --features audio,test-support --test midi_pcm_differential -- --nocapture
//! ```
//!
//! To compare against an earlier capture, also set
//! `DEPS_BATCH_025_PCM_BASELINE_DIR` to its capture directory. The target
//! writes the candidate's raw `*.f32le` captures and a JSON receipt before it
//! fails on any bit or frame-count difference. It never applies tolerance,
//! normalization, clipping, or acceptance. The parent runner hashes the
//! fixture files in `DEPS_BATCH_025_PCM_FIXTURE_DIR` before the version matrix.
//! For a harness self-comparison, first capture to directory A, then rerun
//! the same version, target, and inputs to directory B with A as the baseline.
//!
//! The malformed odd-sized SF2 `smpl` chunk is parsed and production-loaded in
//! an isolated copy of this test process; its raw child output and explicit
//! rejection, panic, or abnormal-exit classification are retained without
//! inventing PCM. A separate SF2 enables continuous sample looping while
//! declaring start >= end; parsing and production loading are recorded, and
//! PCM is rendered only if the production loader accepts that fixture.

use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use client::sound::{Midi, RustyMidi};
use rustysynth::{LoopMode, MidiFile, MidiFileLoopType, SoundFont};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SAMPLE_RATE: usize = 22_050;
const TAIL_FRAMES: usize = 2 * SAMPLE_RATE;
const CHANNELS: usize = 2;
const BYTES_PER_SAMPLE: usize = 4;
const SF2_SAMPLE_FRAMES: usize = 4_096;
const SF2_GUARD_FRAMES: usize = 46;
const LOUD_ATTENUATION_CENTIBELS: i16 = -10_000;
const PROGRAM: u16 = 34;
const BANK: u16 = 0;
const PITCH: u8 = 45;
const VELOCITY: u8 = 100;
const ACCEPTED_LOUD_FONT_SHA256: &str =
    "b12f6c5d183fa351c50c209c0960badae79f49dc760ad1ff87a2fdcf76a66545";
const ACCEPTED_INVALID_LOOP_SHA256: &str =
    "7be33e97ffbf28fa2578ddbb429ad60777c2be5c1e963e075079fb246e2616dc";
const OPERATOR_RULING: &str = "2026-11-01: PR 59 gain cap and PR 60/61 malformed-loop tolerance; DEPS-BATCH-025/impl/matrix-linux/pcm-197/pcm-197/midi-pcm-differential.json";
const PCM_CASES: [&str; 8] = [
    "scape-main",
    "scape-cave",
    "scape-wild",
    "midi-controls",
    "rpgmaker-loop-marked-one-shot",
    "rpgmaker-loop-two-passes",
    "loud-negative-attenuation",
    "zero-attenuation-control",
];

#[derive(Clone)]
struct InputAsset {
    path: &'static str,
    bytes: Vec<u8>,
    sha256: String,
}

struct FrozenFixture {
    name: &'static str,
    path: PathBuf,
    bytes: Vec<u8>,
    sha256: String,
}

struct Fixtures {
    loud_font: FrozenFixture,
    control_font: FrozenFixture,
    loud_midi: FrozenFixture,
    controls_midi: FrozenFixture,
    loop_midi: FrozenFixture,
    invalid_sample_loop_font: FrozenFixture,
    odd_smpl_font: FrozenFixture,
    sample_loop_midi: FrozenFixture,
}

#[derive(Debug)]
struct Qualification {
    parsed_preset_attenuation_db: f32,
    parsed_instrument_attenuation_db: f32,
    parsed_active_attenuation_db: f32,
    old_formula_pre_cap_db: f32,
    sample_count: usize,
    sample_loop_start: i32,
    sample_loop_end: i32,
}

struct Capture {
    id: &'static str,
    requested_frames: usize,
    rendered_frames: usize,
    block_pattern: Vec<usize>,
    block_count: usize,
    final_block_frames: usize,
    play_loop_enabled: bool,
    accepted: bool,
    is_playing_after: bool,
    finite: bool,
    finite_sample_count: usize,
    scalar_sample_count: usize,
    nonzero_sample_count: usize,
    peak_left: f32,
    peak_right: f32,
    pcm: Vec<u8>,
}

struct FixtureCorrections<'a> {
    candidate_version: &'a str,
    gain_cap_reference: &'a [u8],
    control_pcm: &'a [u8],
}

impl FixtureCorrections<'_> {
    fn accepts_transition(&self, baseline_version: &str) -> bool {
        baseline_version == "1.3.6" && matches!(self.candidate_version, "1.3.7" | "1.3.8")
    }

    fn accepts_gain_cap(
        &self,
        baseline_version: &str,
        case: &str,
        candidate: &[u8],
        comparison: &Value,
    ) -> bool {
        self.accepts_transition(baseline_version)
            && case == "loud-negative-attenuation"
            && candidate == self.gain_cap_reference
            && candidate.len() == 88_200 * CHANNELS * BYTES_PER_SAMPLE
            && comparison["frame_count_matches"] == Value::Bool(true)
            && comparison["finite_comparison"] == Value::Bool(true)
            && comparison["changed_sample_count"] == json!(8_192)
    }

    fn accepts_sample_loop(
        &self,
        baseline_version: &str,
        baseline: &Value,
        candidate: &Value,
        candidate_dir: &Path,
    ) -> bool {
        if !self.accepts_transition(baseline_version)
            || baseline["parser_status"] != "rejected"
            || baseline.get("render_capture").is_some()
            || candidate["parser_status"] != "accepted"
            || candidate["production_load_status"] != "accepted"
            || candidate["production_render_status"] != "completed"
            || candidate["fixture_loop_parameters_match"] != Value::Bool(true)
        {
            return false;
        }
        let Some(filename) = candidate["render_capture"]["pcm_file"].as_str() else {
            return false;
        };
        let pcm =
            fs::read(candidate_dir.join(filename)).expect("read accepted invalid-loop fixture PCM");
        pcm.len() == 66_150 * CHANNELS * BYTES_PER_SAMPLE
            && self.control_pcm.get(..pcm.len()) == Some(pcm.as_slice())
    }
}

fn validate_accepted_fixture_identity(fixtures: &Fixtures) {
    assert_eq!(fixtures.loud_font.sha256, ACCEPTED_LOUD_FONT_SHA256);
    assert_eq!(
        fixtures.invalid_sample_loop_font.sha256,
        ACCEPTED_INVALID_LOOP_SHA256
    );
}

fn render_gain_cap_reference(fixtures: &Fixtures) -> Vec<u8> {
    // This one-region SF2 has no velocity-dependent generators. Zero attenuation
    // and velocity 127 produce exactly the accepted 0 dB note gain.
    let mut midi = RustyMidi::with_sound_font(&fixtures.control_font.path.display().to_string())
        .expect("load zero-attenuation gain-cap reference");
    let reference = render_case(
        &mut midi,
        "gain-cap-reference",
        &loud_fixture_midi(127),
        4 * SAMPLE_RATE,
        &[64],
        false,
    );
    assert!(reference.finite && reference.nonzero_sample_count > 0);
    reference.pcm
}

#[test]
fn captures_production_pcm_and_compares_optional_baseline() {
    if std::env::var_os("DEPS_BATCH_025_PCM_ODD_SMPL_CHILD").is_some() {
        let path = required_env_path("DEPS_BATCH_025_PCM_ODD_SMPL_PATH");
        let result = inspect_odd_smpl_in_child(&path);
        println!(
            "DEPS_BATCH_025_ODD_SMPL_RESULT={}",
            serde_json::to_string(&result).expect("serialize isolated odd-smpl outcome")
        );
        return;
    }
    let corpus_root = required_env_path("DEPS_BATCH_025_PCM_CORPUS");
    let capture_dir = required_env_path("DEPS_BATCH_025_PCM_CAPTURE_DIR");
    let fixture_dir = required_env_path("DEPS_BATCH_025_PCM_FIXTURE_DIR");
    fs::create_dir_all(&capture_dir).expect("create PCM capture directory");
    fs::create_dir_all(&fixture_dir).expect("create frozen fixture directory");

    let assets = load_corpus(&corpus_root);
    let fixtures = freeze_fixtures(&fixture_dir);
    validate_accepted_fixture_identity(&fixtures);
    let rustysynth_version = locked_rustysynth_version();
    assert!(
        matches!(rustysynth_version.as_str(), "1.3.6" | "1.3.7" | "1.3.8"),
        "this target is scoped to rustysynth 1.3.6/1.3.7/1.3.8; lockfile selects {rustysynth_version}"
    );
    let odd_smpl_outcome = run_odd_smpl_isolated(&fixtures.odd_smpl_font, &capture_dir);
    let (mut sample_loop_outcome, sample_loop_capture) = probe_invalid_sample_loop(
        &fixtures.invalid_sample_loop_font,
        &fixtures.sample_loop_midi.bytes,
    );
    let mut edge_case_failures = Vec::new();
    if odd_smpl_outcome["classification"].as_str() == Some("child-result-missing") {
        edge_case_failures
            .push("odd-smpl-chunk: isolated child exited without a parse/load result".to_owned());
    }
    if sample_loop_outcome["parser_status"].as_str() == Some("accepted")
        && sample_loop_outcome["parsed_loop_design"]["fixture_loop_parameters_match"]
            != Value::Bool(true)
    {
        edge_case_failures.push(
            "invalid-sample-loop: parser did not preserve the enabled invalid loop design"
                .to_owned(),
        );
    }
    let sample_loop_capture_receipt = sample_loop_capture.as_ref().map(|capture| {
        let filename = format!("{}.f32le", capture.id);
        let path = capture_dir.join(&filename);
        fs::write(&path, &capture.pcm)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        if !capture.accepted
            || !capture.finite
            || capture.nonzero_sample_count == 0
            || capture.is_playing_after
            || capture.rendered_frames != capture.requested_frames
        {
            edge_case_failures.push(format!(
                "{}: accepted invalid sample-loop capture did not meet play-accepted, finite, audible, complete, ended-playback checks",
                capture.id
            ));
        }
        capture_receipt(capture, &filename)
    });
    if let Some(capture_receipt) = sample_loop_capture_receipt {
        sample_loop_outcome["render_capture"] = capture_receipt;
    }

    let loud_qualification = qualify_font(&fixtures.loud_font.bytes, LOUD_ATTENUATION_CENTIBELS);
    let control_qualification = qualify_font(&fixtures.control_font.bytes, 0);
    assert!(
        loud_qualification.old_formula_pre_cap_db > 0.0,
        "the loud fixture must drive the old pre-cap formula above 0 dB: {loud_qualification:?}"
    );
    assert!(
        control_qualification.old_formula_pre_cap_db <= 0.0,
        "the zero-attenuation control must not trigger the old gain-cap condition: {control_qualification:?}"
    );

    let font_path = corpus_root.join("client/SCC1_Florestan.sf2");
    let mut midi = RustyMidi::with_sound_font(&font_path.display().to_string())
        .expect("production RustyMidi must load the hash-pinned SCC1 SoundFont");
    assert!(
        midi.has_sound_font(),
        "explicit production SoundFont loaded"
    );

    let mut captures = Vec::with_capacity(PCM_CASES.len());
    for song in [
        (&assets[2], "scape-main"),
        (&assets[3], "scape-cave"),
        (&assets[4], "scape-wild"),
    ] {
        let frames = complete_song_frames(&song.0.bytes);
        captures.push(render_case(
            &mut midi,
            song.1,
            &song.0.bytes,
            frames,
            &[64],
            false,
        ));
    }

    let controls_frames = complete_song_frames(&fixtures.controls_midi.bytes);
    captures.push(render_case(
        &mut midi,
        "midi-controls",
        &fixtures.controls_midi.bytes,
        controls_frames,
        &[64],
        false,
    ));

    let loop_frames = complete_song_frames(&fixtures.loop_midi.bytes);
    let loop_capture = render_case(
        &mut midi,
        "rpgmaker-loop-marked-one-shot",
        &fixtures.loop_midi.bytes,
        loop_frames,
        &[64],
        false,
    );
    let loop_was_one_shot = !loop_capture.is_playing_after;
    captures.push(loop_capture);

    let two_pass_frames = rpgmaker_two_pass_frames(&fixtures.loop_midi.bytes);
    assert_eq!(
        two_pass_frames, 77_248,
        "fixed two-pass RPG Maker frame count"
    );
    let two_pass_capture = render_case(
        &mut midi,
        "rpgmaker-loop-two-passes",
        &fixtures.loop_midi.bytes,
        two_pass_frames,
        &[64],
        true,
    );
    let two_pass_capture_still_playing = two_pass_capture.is_playing_after;
    assert_eq!(two_pass_capture.block_count, 1_207);
    captures.push(two_pass_capture);

    // The malformed MIDI-marker and odd-frame probes were not SF2 parsing
    // evidence. Invalid sample-loop and odd-smpl chunks are recorded in
    // `sf2_edge_cases` below, separately from the valid-PCM comparisons.
    let loud_frames = 4 * SAMPLE_RATE;
    assert_eq!(loud_frames, 88_200, "fixed loud fixture frame count");
    let mut loud_midi = RustyMidi::with_sound_font(&fixtures.loud_font.path.display().to_string())
        .expect("production RustyMidi must load the generated loud SF2");
    let loud_capture = render_case(
        &mut loud_midi,
        "loud-negative-attenuation",
        &fixtures.loud_midi.bytes,
        loud_frames,
        &[64],
        false,
    );
    assert_eq!(loud_capture.block_count, 1_379);
    assert_eq!(loud_capture.final_block_frames, 8);
    captures.push(loud_capture);

    let mut control_midi =
        RustyMidi::with_sound_font(&fixtures.control_font.path.display().to_string())
            .expect("production RustyMidi must load the generated zero-attenuation SF2");
    let control_capture = render_case(
        &mut control_midi,
        "zero-attenuation-control",
        &fixtures.loud_midi.bytes,
        loud_frames,
        &[64],
        false,
    );
    captures.push(control_capture);

    let corrected_version = matches!(rustysynth_version.as_str(), "1.3.7" | "1.3.8");
    let gain_cap_reference = if corrected_version {
        render_gain_cap_reference(&fixtures)
    } else {
        Vec::new()
    };
    let gain_cap_matches_reference = !corrected_version
        || captures
            .iter()
            .find(|capture| capture.id == "loud-negative-attenuation")
            .expect("loud fixture was captured")
            .pcm
            == gain_cap_reference;
    let corrections = FixtureCorrections {
        candidate_version: &rustysynth_version,
        gain_cap_reference: &gain_cap_reference,
        control_pcm: &captures.last().expect("control fixture was captured").pcm,
    };

    let mut comparison_failures = Vec::new();
    let baseline_dir = optional_env_path("DEPS_BATCH_025_PCM_BASELINE_DIR");
    let (baseline_version, comparisons, baseline_receipt) =
        if let Some(baseline_dir) = baseline_dir.as_ref() {
            let baseline_receipt_path = baseline_dir.join("midi-pcm-differential.json");
            let baseline_receipt_bytes = fs::read(&baseline_receipt_path).unwrap_or_else(|error| {
                panic!(
                    "DEPS_BATCH_025_PCM_BASELINE_DIR requires {}: {error}",
                    baseline_receipt_path.display()
                )
            });
            let baseline_receipt: Value = serde_json::from_slice(&baseline_receipt_bytes)
                .expect("baseline receipt must be valid JSON");
            validate_baseline_identity(&baseline_receipt, &assets, &fixtures);
            let version = baseline_receipt["rustysynth_version"]
                .as_str()
                .expect("baseline receipt must name rustysynth_version")
                .to_owned();
            let mut comparisons = Vec::with_capacity(captures.len());
            for capture in &captures {
                let filename = format!("{}.f32le", capture.id);
                let baseline_pcm_path = baseline_dir.join(&filename);
                let baseline_pcm = fs::read(&baseline_pcm_path).unwrap_or_else(|error| {
                    panic!(
                        "baseline receipt requires {}: {error}",
                        baseline_pcm_path.display()
                    )
                });
                let mut comparison = compare_pcm(&capture.pcm, &baseline_pcm);
                let accepted_gain_cap =
                    corrections.accepts_gain_cap(&version, capture.id, &capture.pcm, &comparison);
                if accepted_gain_cap {
                    comparison["operator_acceptance"] = json!("accepted-PR59-gain-cap");
                    comparison["operator_ruling"] = json!(OPERATOR_RULING);
                    comparison["matches_zero_db_reference"] = json!(true);
                }
                if comparison["exact_bit_match"] != Value::Bool(true) && !accepted_gain_cap {
                    comparison_failures.push(format!(
                        "{}: {}",
                        capture.id,
                        comparison["status"]
                            .as_str()
                            .unwrap_or("invalid comparison")
                    ));
                }
                comparisons.push(json!({
                    "case": capture.id,
                    "baseline_pcm_file": filename,
                    "result": comparison,
                }));
            }
            (Some(version), comparisons, Some(baseline_receipt))
        } else {
            (None, Vec::new(), None)
        };
    let special_case_comparisons = match (baseline_receipt.as_ref(), baseline_dir.as_ref()) {
        (Some(baseline_receipt), Some(baseline_dir)) => compare_sf2_edge_cases(
            baseline_receipt,
            baseline_dir,
            &capture_dir,
            &odd_smpl_outcome,
            &sample_loop_outcome,
            &corrections,
            &mut comparison_failures,
        ),
        _ => Vec::new(),
    };

    let asset_identities: Vec<Value> = assets
        .iter()
        .map(|asset| {
            json!({
                "path": asset.path,
                "sha256": asset.sha256,
                "byte_len": asset.bytes.len(),
            })
        })
        .collect();
    let fixture_identities: Vec<Value> = fixture_list(&fixtures)
        .iter()
        .map(|fixture| {
            json!({
                "name": fixture.name,
                "sha256": fixture.sha256,
                "byte_len": fixture.bytes.len(),
            })
        })
        .collect();

    let mut capture_receipts = Vec::with_capacity(captures.len());
    let mut render_failures = edge_case_failures;
    if !gain_cap_matches_reference {
        render_failures
            .push("loud fixture does not match the exact 0 dB gain-cap reference".into());
    }
    if corrected_version {
        fs::write(
            capture_dir.join("gain-cap-reference.f32le"),
            &gain_cap_reference,
        )
        .expect("write exact gain-cap expectation reference");
    }
    for capture in &captures {
        let filename = format!("{}.f32le", capture.id);
        let path = capture_dir.join(&filename);
        fs::write(&path, &capture.pcm)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        if !capture.finite {
            render_failures.push(format!(
                "{}: {} of {} scalar samples are finite",
                capture.id, capture.finite_sample_count, capture.scalar_sample_count
            ));
        }
        if capture.rendered_frames != capture.requested_frames {
            render_failures.push(format!(
                "{}: rendered {} frames, requested {}",
                capture.id, capture.rendered_frames, capture.requested_frames
            ));
        }
        if matches!(
            capture.id,
            "loud-negative-attenuation" | "zero-attenuation-control" | "rpgmaker-loop-two-passes"
        ) && capture.nonzero_sample_count == 0
        {
            render_failures.push(format!("{}: PCM is all zero", capture.id));
        }
        if matches!(capture.id, "scape-main" | "scape-cave" | "scape-wild")
            && capture.is_playing_after
        {
            render_failures.push(format!(
                "{}: is_playing remained true after the two-second tail",
                capture.id
            ));
        }
        if capture.id == "rpgmaker-loop-marked-one-shot" && capture.is_playing_after {
            render_failures.push(
                "production one-shot RPG Maker probe stayed active beyond its tail".to_owned(),
            );
        }
        if capture.id == "rpgmaker-loop-two-passes" && !capture.is_playing_after {
            render_failures.push(
                "test-support RPG Maker loop stopped before the two-pass capture completed"
                    .to_owned(),
            );
        }
        capture_receipts.push(capture_receipt(capture, &filename));
    }

    let receipt = json!({
        "schema": "deps-batch-025-midi-pcm-differential-v2",
        "rustysynth_version": rustysynth_version,
        "target": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        "sample_rate_hz": SAMPLE_RATE,
        "channels": CHANNELS,
        "sample_format": "f32-le-interleaved-stereo",
        "capture_stage": "RustyMidi::render before mixer fade and audio device",
        "sound_font_selection": {
            "method": "explicit RustyMidi::with_sound_font",
            "corpus_relative_path": "client/SCC1_Florestan.sf2",
            "sha256": assets[0].sha256,
            "cache_search_used": false,
            "working_directory_fallback_used": false,
        },
        "effects": { "reverb": false, "chorus": false },
        "master_volume": 0.5,
        "corpus_root_env": "DEPS_BATCH_025_PCM_CORPUS",
        "asset_identities": asset_identities,
        "fixture_identities": fixture_identities,
        "fixture_design": {
            "preset": { "bank": BANK, "program": PROGRAM },
            "global_initial_attenuation_centibels": LOUD_ATTENUATION_CENTIBELS,
            "generator_table": [
                {
                    "zone": "preset-global",
                    "operation": "initialAttenuation",
                    "generator_id": 48,
                    "signed_centibels": LOUD_ATTENUATION_CENTIBELS,
                },
                {
                    "zone": "preset-local",
                    "operation": "instrument",
                    "generator_id": 41,
                    "index": 0,
                },
                {
                    "zone": "instrument-local",
                    "operation": "sampleID",
                    "generator_id": 53,
                    "index": 0,
                },
            ],
            "zero_attenuation_control_generator_table": [
                {
                    "zone": "preset-global",
                    "operation": "initialAttenuation",
                    "generator_id": 48,
                    "signed_centibels": 0,
                },
                {
                    "zone": "preset-local",
                    "operation": "instrument",
                    "generator_id": 41,
                    "index": 0,
                },
                {
                    "zone": "instrument-local",
                    "operation": "sampleID",
                    "generator_id": 53,
                    "index": 0,
                },
            ],
            "instrument_sample_rate_hz": SAMPLE_RATE,
            "sample_original_key": PITCH,
            "sample_frames_without_guard": SF2_SAMPLE_FRAMES,
            "zero_guard_frames": SF2_GUARD_FRAMES,
            "sample_loop_start": 64,
            "sample_loop_end": 4_032,
            "sample_loop_mode": "disabled",
            "note_channel": 0,
            "note_velocity": VELOCITY,
            "note_off_frame": 2 * SAMPLE_RATE,
            "render_frames": loud_frames,
            "render_blocks_of_64": 1_378,
            "final_block_frames": 8,
            "loud_qualification": {
                "parsed_preset_initial_attenuation_db": loud_qualification.parsed_preset_attenuation_db,
                "parsed_instrument_initial_attenuation_db": loud_qualification.parsed_instrument_attenuation_db,
                "parsed_active_initial_attenuation_db": loud_qualification.parsed_active_attenuation_db,
                "old_formula_pre_cap_db": loud_qualification.old_formula_pre_cap_db,
                "parsed_sample_frames": loud_qualification.sample_count,
                "parsed_sample_loop_start": loud_qualification.sample_loop_start,
                "parsed_sample_loop_end": loud_qualification.sample_loop_end,
            },
            "zero_attenuation_control_qualification": {
                "parsed_preset_initial_attenuation_db": control_qualification.parsed_preset_attenuation_db,
                "parsed_instrument_initial_attenuation_db": control_qualification.parsed_instrument_attenuation_db,
                "parsed_active_initial_attenuation_db": control_qualification.parsed_active_attenuation_db,
                "old_formula_pre_cap_db": control_qualification.old_formula_pre_cap_db,
            },
        },
        "sf2_edge_cases": {
            "odd_smpl_chunk": {
                "fixture": fixtures.odd_smpl_font.name,
                "declared_smpl_bytes": (SF2_SAMPLE_FRAMES + SF2_GUARD_FRAMES) * 2 + 1,
                "declared_size_is_odd": true,
                "isolated_subprocess": true,
                "pcm_generated": false,
                "outcome": odd_smpl_outcome,
                "operator_acceptance": "not-decided",
            },
            "invalid_sample_loop": {
                "fixture": fixtures.invalid_sample_loop_font.name,
                "instrument_generator": {
                    "operation": "sampleModes",
                    "generator_id": 54,
                    "value": 1,
                    "mode": "Continuous",
                },
                "sample_loop_start": 4_032,
                "sample_loop_end": 64,
                "outcome": sample_loop_outcome,
                "pcm_generated": sample_loop_outcome.get("render_capture").is_some(),
                "operator_acceptance": "not-decided",
            },
        },
        "loop_coverage": {
            "rpgmaker_marker_cc": 111,
            "requested_passes": 2,
            "two_pass_capture_produced": true,
            "two_pass_capture_case": "rpgmaker-loop-two-passes",
            "two_pass_capture_frames": two_pass_frames,
            "two_pass_capture_still_playing_at_end": two_pass_capture_still_playing,
            "production_default_one_shot_case": "rpgmaker-loop-marked-one-shot",
            "production_default_reached_eof_after_tail": loop_was_one_shot,
            "production_api_obstacle": "production Midi::play_prepared calls sequencer.play(&midi_file, false); RPG Maker markers are parsed but never start looping",
            "loop_probe_resolution": "test-support-only RustyMidi entry point toggles the loop flag through the same private prepared-play helper",
            "test_support_seam": "audio + test-support feature; reuses production prepare and private prepared-play path",
            "production_default_looping_changed": false,
            "direct_rustysynth_loop_substitute_used": false,
        },
        "captures": capture_receipts,
        "accepted_fixture_expectations": {
            "operator_ruling": OPERATOR_RULING,
            "loud_font_sha256": ACCEPTED_LOUD_FONT_SHA256,
            "invalid_loop_font_sha256": ACCEPTED_INVALID_LOOP_SHA256,
            "gain_cap_reference_velocity": 127,
            "gain_cap_reference_applied": corrected_version,
            "gain_cap_matches_exact_reference": gain_cap_matches_reference,
        },
        "comparison": {
            "baseline_version": baseline_version,
            "exact_bit_comparison_only": true,
            "tolerance_applied": false,
            "differences_automatically_accepted": false,
            "results": comparisons,
            "sf2_edge_cases": special_case_comparisons,
        },
        "qualification_failures": render_failures,
        "comparison_failures": comparison_failures,
    });
    let receipt_path = capture_dir.join("midi-pcm-differential.json");
    let mut receipt_bytes = serde_json::to_vec_pretty(&receipt).expect("serialize PCM receipt");
    receipt_bytes.push(b'\n');
    fs::write(&receipt_path, &receipt_bytes)
        .unwrap_or_else(|error| panic!("write {}: {error}", receipt_path.display()));
    println!("PCM differential receipt: {}", receipt_path.display());

    assert!(
        render_failures.is_empty(),
        "PCM/edge-case qualification failed; see {}: {}",
        receipt_path.display(),
        render_failures.join("; ")
    );
    assert!(
        comparison_failures.is_empty(),
        "PCM or SF2 parser/load outcome differs from baseline; differences require operator ruling and are not accepted: see {}: {}",
        receipt_path.display(),
        comparison_failures.join("; ")
    );
}

#[test]
#[ignore = "compare retained PCM without replaying the unchanged game-song renderer"]
fn retained_pcm_comparison_uses_operator_ruling() {
    let candidate_dir = required_env_path("DEPS_BATCH_025_PCM_EXISTING_CAPTURE_DIR");
    let baseline_dir = required_env_path("DEPS_BATCH_025_PCM_BASELINE_DIR");
    let assets = load_corpus(&required_env_path("DEPS_BATCH_025_PCM_CORPUS"));
    let fixtures = freeze_fixtures(&required_env_path("DEPS_BATCH_025_PCM_FIXTURE_DIR"));
    validate_accepted_fixture_identity(&fixtures);
    let read_receipt = |root: &Path| -> Value {
        serde_json::from_slice(
            &fs::read(root.join("midi-pcm-differential.json")).expect("read retained PCM receipt"),
        )
        .expect("parse retained PCM receipt")
    };
    let candidate_receipt = read_receipt(&candidate_dir);
    let baseline_receipt = read_receipt(&baseline_dir);
    validate_baseline_identity(&candidate_receipt, &assets, &fixtures);
    validate_baseline_identity(&baseline_receipt, &assets, &fixtures);
    let candidate_version = locked_rustysynth_version();
    assert_eq!(candidate_receipt["rustysynth_version"], candidate_version);
    let baseline_version = baseline_receipt["rustysynth_version"]
        .as_str()
        .expect("retained baseline version");
    let control_pcm = fs::read(candidate_dir.join("zero-attenuation-control.f32le"))
        .expect("read retained control PCM");
    let gain_cap_reference = render_gain_cap_reference(&fixtures);
    let corrections = FixtureCorrections {
        candidate_version: &candidate_version,
        gain_cap_reference: &gain_cap_reference,
        control_pcm: &control_pcm,
    };
    assert_eq!(
        candidate_receipt["captures"]
            .as_array()
            .expect("candidate cases")
            .len(),
        PCM_CASES.len()
    );
    assert_eq!(
        baseline_receipt["captures"]
            .as_array()
            .expect("baseline cases")
            .len(),
        PCM_CASES.len()
    );
    let mut failures = Vec::new();
    let mut comparisons = Vec::with_capacity(PCM_CASES.len());
    for case in PCM_CASES {
        let filename = format!("{case}.f32le");
        let candidate_storage = if case == "zero-attenuation-control" {
            None
        } else {
            Some(fs::read(candidate_dir.join(&filename)).expect("read retained candidate PCM"))
        };
        let candidate = candidate_storage.as_deref().unwrap_or(&control_pcm);
        let baseline = fs::read(baseline_dir.join(&filename)).expect("read retained baseline PCM");
        for (receipt, bytes) in [
            (&candidate_receipt, candidate),
            (&baseline_receipt, baseline.as_slice()),
        ] {
            let row = receipt["captures"]
                .as_array()
                .expect("retained case list")
                .iter()
                .find(|row| row["case"] == case)
                .expect("each retained case is present");
            assert_eq!(row["pcm_file"], filename);
            assert_eq!(row["pcm_sha256"], sha256(bytes));
        }
        let mut comparison = compare_pcm(candidate, &baseline);
        let accepted_gain_cap =
            corrections.accepts_gain_cap(baseline_version, case, candidate, &comparison);
        if accepted_gain_cap {
            comparison["operator_acceptance"] = json!("accepted-PR59-gain-cap");
            comparison["operator_ruling"] = json!(OPERATOR_RULING);
            comparison["matches_zero_db_reference"] = json!(true);
        } else if comparison["exact_bit_match"] != Value::Bool(true) {
            failures.push(format!("{case}: unapproved PCM difference"));
        }
        comparisons.push(json!({"case": case, "result": comparison}));
    }
    let special_cases = compare_sf2_edge_cases(
        &baseline_receipt,
        &baseline_dir,
        &candidate_dir,
        &candidate_receipt["sf2_edge_cases"]["odd_smpl_chunk"]["outcome"],
        &candidate_receipt["sf2_edge_cases"]["invalid_sample_loop"]["outcome"],
        &corrections,
        &mut failures,
    );
    let result = json!({
        "result": if failures.is_empty() { "PASS" } else { "FAIL" },
        "operator_ruling": OPERATOR_RULING,
        "baseline_version": baseline_version,
        "candidate_version": candidate_version,
        "loud_fixture_sha256": ACCEPTED_LOUD_FONT_SHA256,
        "invalid_loop_fixture_sha256": ACCEPTED_INVALID_LOOP_SHA256,
        "gain_cap_reference_sha256": sha256(&gain_cap_reference),
        "comparison": comparisons,
        "sf2_edge_cases": special_cases,
        "failures": failures,
        "tolerance_applied": false,
    });
    let path = candidate_dir.join("operator-ruling-comparison.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&result).expect("serialize ruling comparison"),
    )
    .expect("write ruling comparison");
    println!("Retained PCM ruling comparison: {}", path.display());
    assert!(
        failures.is_empty(),
        "unapproved retained PCM differences: {failures:?}"
    );
}

fn required_env_path(name: &str) -> PathBuf {
    let value = std::env::var_os(name).unwrap_or_else(|| {
        panic!("{name} must name an existing input or explicit output directory")
    });
    PathBuf::from(value)
}

fn optional_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn load_corpus(root: &Path) -> Vec<InputAsset> {
    [
        (
            "client/SCC1_Florestan.sf2",
            "6cc153f925bc3c0136d431802bd0039587dd3b3ef4a05b2e9d316c48f41a0363",
        ),
        (
            "bot/SCC1_Florestan.sf2",
            "6cc153f925bc3c0136d431802bd0039587dd3b3ef4a05b2e9d316c48f41a0363",
        ),
        (
            "songs/scape main.mid",
            "1aad4dd3646cb89e66becba53a23cf6a837f29a7b86f7472fbc7e8650e79cc4b",
        ),
        (
            "songs/scape cave.mid",
            "777684871549f8076b94a88830949886284d37b2548334d05294cf01af8a216f",
        ),
        (
            "songs/scape wild.mid",
            "3525b099183af7e86f1d05be4a917d516988b2978a8b194ef44cc20ba3aea587",
        ),
    ]
    .into_iter()
    .map(|(relative_path, expected_sha256)| {
        let path = root.join(relative_path);
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            panic!("required PCM corpus asset {}: {error}", path.display())
        });
        let actual_sha256 = sha256(&bytes);
        assert_eq!(
            actual_sha256,
            expected_sha256,
            "frozen PCM corpus asset {} has the wrong SHA-256",
            path.display()
        );
        InputAsset {
            path: relative_path,
            bytes,
            sha256: actual_sha256,
        }
    })
    .collect()
}

fn freeze_fixtures(root: &Path) -> Fixtures {
    let loud_font_bytes = build_sf2(LOUD_ATTENUATION_CENTIBELS);
    let control_font_bytes = build_sf2(0);
    let invalid_sample_loop_font_bytes = build_invalid_sample_loop_sf2();
    let odd_smpl_font_bytes = build_odd_smpl_sf2();
    let loud_midi_bytes = loud_fixture_midi(VELOCITY);
    let controls_midi_bytes = controls_midi();
    let loop_midi_bytes = rpgmaker_loop_midi();
    let sample_loop_midi_bytes = sample_loop_midi();

    Fixtures {
        loud_font: freeze_fixture(root, "loud-negative-attenuation.sf2", &loud_font_bytes),
        control_font: freeze_fixture(root, "zero-attenuation-control.sf2", &control_font_bytes),
        loud_midi: freeze_fixture(root, "loud-fixture-note.mid", &loud_midi_bytes),
        controls_midi: freeze_fixture(root, "midi-controls.mid", &controls_midi_bytes),
        loop_midi: freeze_fixture(root, "rpgmaker-loop-marked.mid", &loop_midi_bytes),
        invalid_sample_loop_font: freeze_fixture(
            root,
            "invalid-sample-loop.sf2",
            &invalid_sample_loop_font_bytes,
        ),
        odd_smpl_font: freeze_fixture(root, "odd-smpl-chunk.sf2", &odd_smpl_font_bytes),
        sample_loop_midi: freeze_fixture(
            root,
            "invalid-sample-loop-note.mid",
            &sample_loop_midi_bytes,
        ),
    }
}

fn freeze_fixture(root: &Path, name: &'static str, generated: &[u8]) -> FrozenFixture {
    let path = root.join(name);
    let bytes = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(generated)
                .unwrap_or_else(|error| panic!("write frozen fixture {}: {error}", path.display()));
            file.sync_all()
                .unwrap_or_else(|error| panic!("sync frozen fixture {}: {error}", path.display()));
            generated.to_vec()
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read(&path).unwrap_or_else(|read_error| {
                panic!("read frozen fixture {}: {read_error}", path.display())
            });
            assert_eq!(
                existing,
                generated,
                "frozen fixture {} does not match the deterministic generator",
                path.display()
            );
            existing
        }
        Err(error) => panic!("create frozen fixture {}: {error}", path.display()),
    };
    let sha256 = sha256(&bytes);
    FrozenFixture {
        name,
        path,
        bytes,
        sha256,
    }
}

fn fixture_list(fixtures: &Fixtures) -> Vec<&FrozenFixture> {
    vec![
        &fixtures.loud_font,
        &fixtures.control_font,
        &fixtures.loud_midi,
        &fixtures.controls_midi,
        &fixtures.loop_midi,
        &fixtures.invalid_sample_loop_font,
        &fixtures.odd_smpl_font,
        &fixtures.sample_loop_midi,
    ]
}

fn qualify_font(bytes: &[u8], expected_attenuation_centibels: i16) -> Qualification {
    let mut reader = Cursor::new(bytes);
    let sound_font = SoundFont::new(&mut reader).expect("parse self-authored SF2 fixture");
    assert_eq!(
        sound_font.get_wave_data().len(),
        SF2_SAMPLE_FRAMES + SF2_GUARD_FRAMES
    );
    assert_eq!(sound_font.get_bits_per_sample(), 16);
    assert_eq!(sound_font.get_presets().len(), 1);
    assert_eq!(sound_font.get_instruments().len(), 1);

    let preset = sound_font
        .get_presets()
        .iter()
        .find(|preset| {
            preset.get_bank_number() == i32::from(BANK)
                && preset.get_patch_number() == i32::from(PROGRAM)
        })
        .expect("fixture contains bank 0 program 34 preset");
    let active_preset_region = preset
        .get_regions()
        .iter()
        .find(|region| region.contains(i32::from(PITCH), i32::from(VELOCITY)))
        .expect("fixture preset region covers key 45 and velocity 100");
    assert_eq!(preset.get_regions().len(), 1);
    assert_eq!(active_preset_region.get_instrument_id(), 0);

    let instrument = sound_font
        .get_instruments()
        .get(active_preset_region.get_instrument_id())
        .expect("active preset region references the fixture instrument");
    let active_instrument_region = instrument
        .get_regions()
        .iter()
        .find(|region| region.contains(i32::from(PITCH), i32::from(VELOCITY)))
        .expect("fixture instrument region covers key 45 and velocity 100");
    assert_eq!(instrument.get_regions().len(), 1);
    assert_eq!(
        active_instrument_region.get_sample_modes(),
        LoopMode::NoLoop
    );

    let parsed_preset_attenuation_db = active_preset_region.get_initial_attenuation();
    let parsed_instrument_attenuation_db = active_instrument_region.get_initial_attenuation();
    let parsed_active_attenuation_db =
        parsed_preset_attenuation_db + parsed_instrument_attenuation_db;
    assert_eq!(
        parsed_preset_attenuation_db,
        f32::from(expected_attenuation_centibels) / 10.0,
        "RustySynth must parse the active preset generator as signed centibels"
    );
    assert_eq!(parsed_instrument_attenuation_db, 0.0);

    let sample = sound_font
        .get_sample_headers()
        .first()
        .expect("fixture has one sample header");
    assert_eq!(sample.get_start(), 0);
    assert_eq!(sample.get_end(), SF2_SAMPLE_FRAMES as i32);
    assert_eq!(sample.get_start_loop(), 64);
    assert_eq!(sample.get_end_loop(), 4_032);
    assert_eq!(sample.get_sample_rate(), SAMPLE_RATE as i32);
    assert_eq!(sample.get_original_pitch(), i32::from(PITCH));
    assert_eq!(sample.get_sample_type(), 1);

    let filter_q = active_preset_region.get_initial_filter_q()
        + active_instrument_region.get_initial_filter_q();
    let velocity_db = 20.0_f32 * (f32::from(VELOCITY) / 127.0).log10();
    let old_formula_pre_cap_db =
        2.0 * velocity_db - 0.4 * parsed_active_attenuation_db - 0.5 * filter_q;

    Qualification {
        parsed_preset_attenuation_db,
        parsed_instrument_attenuation_db,
        parsed_active_attenuation_db,
        old_formula_pre_cap_db,
        sample_count: sound_font.get_wave_data().len(),
        sample_loop_start: sample.get_start_loop(),
        sample_loop_end: sample.get_end_loop(),
    }
}

fn render_case(
    midi: &mut RustyMidi,
    id: &'static str,
    midi_bytes: &[u8],
    requested_frames: usize,
    block_pattern: &[usize],
    play_loop_enabled: bool,
) -> Capture {
    assert!(
        !block_pattern.is_empty(),
        "render schedule must contain a block size"
    );
    assert!(block_pattern.iter().all(|size| (1..=64).contains(size)));
    midi.stop();
    let accepted = if play_loop_enabled {
        midi.play_rpgmaker_with_loop_for_test(midi_bytes, true)
    } else {
        midi.play(midi_bytes, 0, false)
    };
    assert!(accepted, "RustyMidi playback rejected {id}");

    let mut pcm = Vec::with_capacity(requested_frames * CHANNELS * BYTES_PER_SAMPLE);
    let mut left = [0.0_f32; 64];
    let mut right = [0.0_f32; 64];
    let mut rendered_frames = 0;
    let mut block_count = 0;
    let mut final_block_frames = 0;
    let mut finite_sample_count = 0;
    let mut scalar_sample_count = 0;
    let mut nonzero_sample_count = 0;
    let mut peak_left = 0.0_f32;
    let mut peak_right = 0.0_f32;
    let mut finite = true;

    while rendered_frames < requested_frames {
        let scheduled = block_pattern[block_count % block_pattern.len()];
        let frames = scheduled.min(requested_frames - rendered_frames);
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        midi.render(&mut left[..frames], &mut right[..frames]);
        for frame in 0..frames {
            let l = left[frame];
            let r = right[frame];
            finite &= l.is_finite() && r.is_finite();
            finite_sample_count += usize::from(l.is_finite()) + usize::from(r.is_finite());
            scalar_sample_count += CHANNELS;
            nonzero_sample_count += usize::from(l != 0.0) + usize::from(r != 0.0);
            peak_left = peak_left.max(l.abs());
            peak_right = peak_right.max(r.abs());
            pcm.extend_from_slice(&l.to_le_bytes());
            pcm.extend_from_slice(&r.to_le_bytes());
        }
        rendered_frames += frames;
        block_count += 1;
        final_block_frames = frames;
    }

    Capture {
        id,
        requested_frames,
        rendered_frames,
        block_pattern: block_pattern.to_vec(),
        block_count,
        final_block_frames,
        play_loop_enabled,
        accepted,
        is_playing_after: midi.is_playing(),
        finite,
        finite_sample_count,
        scalar_sample_count,
        nonzero_sample_count,
        peak_left,
        peak_right,
        pcm,
    }
}

fn capture_receipt(capture: &Capture, filename: &str) -> Value {
    json!({
        "case": capture.id,
        "pcm_file": filename,
        "pcm_sha256": sha256(&capture.pcm),
        "frames_requested": capture.requested_frames,
        "frames_rendered": capture.rendered_frames,
        "sample_rate_hz": SAMPLE_RATE,
        "channels": CHANNELS,
        "sample_format": "f32-le-interleaved-stereo",
        "render_block_pattern_frames": capture.block_pattern,
        "render_block_count": capture.block_count,
        "final_block_frames": capture.final_block_frames,
        "play_loop_enabled_by_capture": capture.play_loop_enabled,
        "play_entrypoint": if capture.play_loop_enabled {
            "RustyMidi::play_rpgmaker_with_loop_for_test"
        } else {
            "Midi::play"
        },
        "accepted_by_rustymidi": capture.accepted,
        "is_playing_after_render": capture.is_playing_after,
        "finite": capture.finite,
        "finite_sample_count": capture.finite_sample_count,
        "scalar_sample_count": capture.scalar_sample_count,
        "nonzero_sample_count": capture.nonzero_sample_count,
        "peak_left": capture.peak_left,
        "peak_right": capture.peak_right,
    })
}

fn probe_invalid_sample_loop(font: &FrozenFixture, midi_bytes: &[u8]) -> (Value, Option<Capture>) {
    let mut reader = Cursor::new(font.bytes.as_slice());
    let sound_font = match catch_unwind(AssertUnwindSafe(|| SoundFont::new(&mut reader))) {
        Err(payload) => {
            return (
                json!({
                    "status": "parser-panic",
                    "parser_status": "panicked",
                    "panic_payload": panic_payload_text(payload.as_ref()),
                }),
                None,
            );
        }
        Ok(Err(error)) => {
            return (
                json!({
                    "status": "parse-rejected",
                    "parser_status": "rejected",
                    "parse_error": error.to_string(),
                }),
                None,
            );
        }
        Ok(Ok(sound_font)) => sound_font,
    };

    let parsed_loop_design = inspect_invalid_sample_loop_design(&sound_font);
    drop(sound_font);
    let font_path = font.path.display().to_string();
    match catch_unwind(AssertUnwindSafe(|| RustyMidi::with_sound_font(&font_path))) {
        Err(payload) => (
            json!({
                "status": "production-load-panic",
                "parser_status": "accepted",
                "production_load_status": "panicked",
                "parsed_loop_design": parsed_loop_design,
                "panic_payload": panic_payload_text(payload.as_ref()),
            }),
            None,
        ),
        Ok(None) => (
            json!({
                "status": "production-load-rejected-after-parse-accepted",
                "parser_status": "accepted",
                "production_load_status": "rejected",
                "parsed_loop_design": parsed_loop_design,
            }),
            None,
        ),
        Ok(Some(mut midi)) => {
            let requested_frames = complete_song_frames(midi_bytes);
            match catch_unwind(AssertUnwindSafe(|| {
                render_case(
                    &mut midi,
                    "invalid-sample-loop",
                    midi_bytes,
                    requested_frames,
                    &[64],
                    false,
                )
            })) {
                Err(payload) => (
                    json!({
                        "status": "production-render-panic",
                        "parser_status": "accepted",
                        "production_load_status": "accepted",
                        "production_render_status": "panicked",
                        "parsed_loop_design": parsed_loop_design,
                        "panic_payload": panic_payload_text(payload.as_ref()),
                    }),
                    None,
                ),
                Ok(capture) => {
                    let fixture_loop_parameters_match =
                        parsed_loop_design["fixture_loop_parameters_match"].as_bool();
                    (
                        json!({
                            "status": "rendered",
                            "parser_status": "accepted",
                            "production_load_status": "accepted",
                            "production_render_status": "completed",
                            "production_play_accepted": capture.accepted,
                            "parsed_loop_design": parsed_loop_design,
                            "fixture_loop_parameters_match": fixture_loop_parameters_match,
                        }),
                        Some(capture),
                    )
                }
            }
        }
    }
}

fn inspect_invalid_sample_loop_design(sound_font: &SoundFont) -> Value {
    let preset = sound_font
        .get_presets()
        .iter()
        .find(|preset| {
            preset.get_bank_number() == i32::from(BANK)
                && preset.get_patch_number() == i32::from(PROGRAM)
        })
        .expect("invalid-loop fixture contains bank 0 program 34 preset");
    let preset_region = preset
        .get_regions()
        .iter()
        .find(|region| region.contains(i32::from(PITCH), i32::from(VELOCITY)))
        .expect("invalid-loop preset region covers key 45 and velocity 100");
    let instrument = sound_font
        .get_instruments()
        .get(preset_region.get_instrument_id())
        .expect("invalid-loop preset region references the fixture instrument");
    let instrument_region = instrument
        .get_regions()
        .iter()
        .find(|region| region.contains(i32::from(PITCH), i32::from(VELOCITY)))
        .expect("invalid-loop instrument region covers key 45 and velocity 100");
    let sample = sound_font
        .get_sample_headers()
        .first()
        .expect("invalid-loop fixture has one sample header");
    let sample_modes_continuous = instrument_region.get_sample_modes() == LoopMode::Continuous;
    let loop_start = sample.get_start_loop();
    let loop_end = sample.get_end_loop();
    json!({
        "sample_modes_continuous": sample_modes_continuous,
        "sample_loop_start": loop_start,
        "sample_loop_end": loop_end,
        "fixture_loop_parameters_match": sample_modes_continuous
            && loop_start == 4_032
            && loop_end == 64
            && loop_start >= loop_end,
    })
}

fn run_odd_smpl_isolated(font: &FrozenFixture, capture_dir: &Path) -> Value {
    const CHILD_RESULT_PREFIX: &str = "DEPS_BATCH_025_ODD_SMPL_RESULT=";
    let output = Command::new(
        std::env::current_exe().expect("resolve current differential-test executable"),
    )
    .current_dir(capture_dir)
    .args([
        "--exact",
        "captures_production_pcm_and_compares_optional_baseline",
        "--nocapture",
    ])
    .env("DEPS_BATCH_025_PCM_ODD_SMPL_CHILD", "1")
    .env("DEPS_BATCH_025_PCM_ODD_SMPL_PATH", &font.path)
    .output()
    .unwrap_or_else(|error| panic!("spawn isolated odd-smpl parser: {error}"));
    let stdout_path = capture_dir.join("odd-smpl-child.stdout");
    let stderr_path = capture_dir.join("odd-smpl-child.stderr");
    fs::write(&stdout_path, &output.stdout)
        .unwrap_or_else(|error| panic!("write {}: {error}", stdout_path.display()));
    fs::write(&stderr_path, &output.stderr)
        .unwrap_or_else(|error| panic!("write {}: {error}", stderr_path.display()));
    let stdout_text = String::from_utf8_lossy(&output.stdout);
    let marker = stdout_text
        .lines()
        .find_map(|line| line.strip_prefix(CHILD_RESULT_PREFIX));
    let child_result = marker.and_then(|line| serde_json::from_str::<Value>(line).ok());
    let decode_error = marker.and_then(|line| {
        serde_json::from_str::<Value>(line)
            .err()
            .map(|error| error.to_string())
    });
    let classification = child_result
        .as_ref()
        .and_then(|result| result["status"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if output.status.success() {
                "child-result-missing".to_owned()
            } else {
                "abnormal-exit".to_owned()
            }
        });
    json!({
        "fixture": font.name,
        "isolated_subprocess": true,
        "pcm_generated": false,
        "classification": classification,
        "exit_success": output.status.success(),
        "exit_code": output.status.code(),
        "signal": process_signal(&output.status),
        "child_result": child_result,
        "child_result_decode_error": decode_error,
        "stdout_file": stdout_path.file_name().and_then(|name| name.to_str()),
        "stdout_sha256": sha256(&output.stdout),
        "stdout_byte_len": output.stdout.len(),
        "stderr_file": stderr_path.file_name().and_then(|name| name.to_str()),
        "stderr_sha256": sha256(&output.stderr),
        "stderr_byte_len": output.stderr.len(),
    })
}

fn inspect_odd_smpl_in_child(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        panic!("read isolated odd-smpl fixture {}: {error}", path.display())
    });
    let mut reader = Cursor::new(bytes.as_slice());
    let parse_result = match catch_unwind(AssertUnwindSafe(|| SoundFont::new(&mut reader))) {
        Err(payload) => {
            return json!({
                "status": "parser-panic",
                "parse_panic_payload": panic_payload_text(payload.as_ref()),
            });
        }
        Ok(Err(error)) => {
            return json!({
                "status": "parse-rejected",
                "parse_error": error.to_string(),
                "fixture_byte_len": bytes.len(),
                "fixture_sha256": sha256(&bytes),
            });
        }
        Ok(Ok(sound_font)) => sound_font,
    };
    drop(parse_result);
    let font_path = path.display().to_string();
    match catch_unwind(AssertUnwindSafe(|| RustyMidi::with_sound_font(&font_path))) {
        Err(payload) => json!({
            "status": "production-load-panic-after-parse-accepted",
            "parse_status": "accepted",
            "production_load_panic_payload": panic_payload_text(payload.as_ref()),
        }),
        Ok(None) => json!({
            "status": "production-load-rejected-after-parse-accepted",
            "parse_status": "accepted",
        }),
        Ok(Some(_midi)) => json!({
            "status": "production-load-accepted",
            "parse_status": "accepted",
            "pcm_generated": false,
        }),
    }
}

fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&'static str>()
                .map(|message| (*message).to_owned())
        })
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

fn process_signal(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

fn compare_sf2_edge_cases(
    baseline_receipt: &Value,
    baseline_dir: &Path,
    candidate_dir: &Path,
    odd_smpl_candidate: &Value,
    sample_loop_candidate: &Value,
    corrections: &FixtureCorrections<'_>,
    failures: &mut Vec<String>,
) -> Vec<Value> {
    let baseline_edges = &baseline_receipt["sf2_edge_cases"];
    let baseline_odd_smpl = &baseline_edges["odd_smpl_chunk"]["outcome"];
    validate_odd_smpl_streams(odd_smpl_candidate, candidate_dir);
    validate_odd_smpl_streams(&baseline_edges["odd_smpl_chunk"]["outcome"], baseline_dir);
    let candidate_odd_signature = odd_smpl_signature(odd_smpl_candidate);
    let baseline_odd_signature = odd_smpl_signature(baseline_odd_smpl);
    let odd_outcome_match = candidate_odd_signature == baseline_odd_signature;
    if !odd_outcome_match {
        failures.push("odd-smpl-chunk: parser/load outcome differs from baseline".to_owned());
    }

    let baseline_sample_loop = &baseline_edges["invalid_sample_loop"]["outcome"];
    let accepted_loop = corrections.accepts_sample_loop(
        baseline_receipt["rustysynth_version"]
            .as_str()
            .expect("baseline version"),
        baseline_sample_loop,
        sample_loop_candidate,
        candidate_dir,
    );
    let candidate_loop_signature = sample_loop_signature(sample_loop_candidate);
    let baseline_loop_signature = sample_loop_signature(baseline_sample_loop);
    let loop_outcome_match = candidate_loop_signature == baseline_loop_signature;
    if !loop_outcome_match && !accepted_loop {
        failures.push("invalid-sample-loop: parser/load outcome differs from baseline".to_owned());
    }

    let candidate_pcm_file = sample_loop_candidate["render_capture"]["pcm_file"].as_str();
    let baseline_pcm_file = baseline_sample_loop["render_capture"]["pcm_file"].as_str();
    let pcm_comparison = match (candidate_pcm_file, baseline_pcm_file) {
        (Some(candidate_file), Some(baseline_file)) => {
            let candidate_path = candidate_dir.join(candidate_file);
            let baseline_path = baseline_dir.join(baseline_file);
            let candidate_pcm = fs::read(&candidate_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", candidate_path.display()));
            let baseline_pcm = fs::read(&baseline_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", baseline_path.display()));
            let comparison = compare_pcm(&candidate_pcm, &baseline_pcm);
            if comparison["exact_bit_match"] != Value::Bool(true) {
                failures.push(format!(
                    "invalid-sample-loop PCM: {}",
                    comparison["status"]
                        .as_str()
                        .unwrap_or("invalid comparison")
                ));
            }
            Some(comparison)
        }
        (None, None) => None,
        _ => {
            if !accepted_loop {
                failures.push(
                    "invalid-sample-loop: one version rendered PCM and the other did not"
                        .to_owned(),
                );
            }
            None
        }
    };
    vec![
        json!({
            "case": "odd-smpl-chunk",
            "candidate_signature": candidate_odd_signature,
            "baseline_signature": baseline_odd_signature,
            "same_outcome": odd_outcome_match,
            "operator_acceptance": "not-decided",
        }),
        json!({
            "case": "invalid-sample-loop",
            "candidate_signature": candidate_loop_signature,
            "baseline_signature": baseline_loop_signature,
            "same_outcome": loop_outcome_match,
            "pcm_comparison": pcm_comparison,
            "operator_acceptance": if accepted_loop { "accepted-PR60-61-malformed-loop" } else { "not-decided" },
            "operator_ruling": if accepted_loop { Some(OPERATOR_RULING) } else { None },
        }),
    ]
}

fn validate_odd_smpl_streams(outcome: &Value, root: &Path) {
    for (file_field, hash_field, length_field, filename) in [
        (
            "stdout_file",
            "stdout_sha256",
            "stdout_byte_len",
            "odd-smpl-child.stdout",
        ),
        (
            "stderr_file",
            "stderr_sha256",
            "stderr_byte_len",
            "odd-smpl-child.stderr",
        ),
    ] {
        assert_eq!(outcome[file_field].as_str(), Some(filename));
        let path = root.join(filename);
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            panic!("read isolated parser output {}: {error}", path.display())
        });
        let actual_sha256 = sha256(&bytes);
        assert_eq!(
            outcome[hash_field].as_str(),
            Some(actual_sha256.as_str()),
            "isolated parser output hash for {}",
            path.display()
        );
        assert_eq!(
            outcome[length_field].as_u64(),
            Some(u64::try_from(bytes.len()).expect("captured output length fits u64")),
            "isolated parser output byte length for {}",
            path.display()
        );
    }
}

fn odd_smpl_signature(outcome: &Value) -> Value {
    json!({
        "classification": outcome["classification"],
        "exit_success": outcome["exit_success"],
        "exit_code": outcome["exit_code"],
        "signal": outcome["signal"],
        // Error text is diagnostic, not a changed rejection or process outcome.
        "child_result": {
            "fixture_byte_len": outcome["child_result"]["fixture_byte_len"],
            "fixture_sha256": outcome["child_result"]["fixture_sha256"],
            "status": outcome["child_result"]["status"],
            "parse_status": outcome["child_result"]["parse_status"],
            "pcm_generated": outcome["child_result"]["pcm_generated"],
        },
    })
}

fn sample_loop_signature(outcome: &Value) -> Value {
    json!({
        "status": outcome["status"],
        "parser_status": outcome["parser_status"],
        "parse_error": outcome["parse_error"],
        "panic_payload": outcome["panic_payload"],
        "production_load_status": outcome["production_load_status"],
        "production_render_status": outcome["production_render_status"],
        "production_play_accepted": outcome["production_play_accepted"],
        "fixture_loop_parameters_match": outcome["fixture_loop_parameters_match"],
        "parsed_loop_design": outcome["parsed_loop_design"],
    })
}

fn complete_song_frames(midi_bytes: &[u8]) -> usize {
    let length_seconds = midi_length_seconds(midi_bytes);
    assert!(length_seconds.is_finite() && length_seconds > 0.0);
    let song_frames = (length_seconds * SAMPLE_RATE as f64).ceil() as usize;
    song_frames
        .checked_add(TAIL_FRAMES)
        .expect("song frame count plus fixed tail fits usize")
}

fn midi_length_seconds(midi_bytes: &[u8]) -> f64 {
    let mut cursor = Cursor::new(midi_bytes);
    MidiFile::new_with_loop_type(&mut cursor, MidiFileLoopType::RpgMaker)
        .expect("production RPG Maker MIDI parser accepts fixed MIDI fixture")
        .get_length()
}

fn rpgmaker_two_pass_frames(midi_bytes: &[u8]) -> usize {
    const SEQUENCER_BLOCK_FRAMES: usize = 64;
    const LOOP_START_FRAMES: usize = SAMPLE_RATE / 2;

    let end_frames = (midi_length_seconds(midi_bytes) * SAMPLE_RATE as f64).round() as usize;
    assert_eq!(
        end_frames,
        2 * SAMPLE_RATE,
        "fixed loop fixture ends at 2 s"
    );

    // RustySynth advances event time by one 64-frame internal block before
    // rendering each block. Stop at the boundary where the second EOT would
    // be processed, so the capture contains the initial pass and one loop
    // pass, but no samples from a third pass.
    let first_eof_block = end_frames.div_ceil(SEQUENCER_BLOCK_FRAMES) * SEQUENCER_BLOCK_FRAMES;
    let loop_marker_block = first_eof_block + SEQUENCER_BLOCK_FRAMES;
    let time_at_loop_marker_block = LOOP_START_FRAMES + SEQUENCER_BLOCK_FRAMES;
    let blocks_to_second_eof =
        (end_frames - time_at_loop_marker_block).div_ceil(SEQUENCER_BLOCK_FRAMES);
    loop_marker_block + blocks_to_second_eof * SEQUENCER_BLOCK_FRAMES
}

fn loud_fixture_midi(velocity: u8) -> Vec<u8> {
    build_midi(
        vec![
            (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
            (0, vec![0xc0, PROGRAM as u8]),
            (0, vec![0xb0, 7, 100]),
            (0, vec![0x90, PITCH, velocity]),
            (1_920, vec![0x80, PITCH, 0]),
        ],
        3_840,
    )
}

fn controls_midi() -> Vec<u8> {
    build_midi(
        vec![
            (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
            (0, vec![0xc0, PROGRAM as u8]),
            (0, vec![0xb0, 7, 112]),
            (0, vec![0xb0, 64, 127]),
            (0, vec![0xe0, 0, 64]),
            (0, vec![0x90, PITCH, VELOCITY]),
            (480, vec![0xe0, 0, 96]),
            (960, vec![0x80, PITCH, 64]),
            (960, vec![0xb0, 7, 78]),
            (1_200, vec![0xb0, 64, 0]),
            (1_440, vec![0xc0, PROGRAM as u8]),
            (1_440, vec![0xe0, 0, 64]),
            (1_440, vec![0x90, PITCH, 80]),
            (1_920, vec![0x80, PITCH, 0]),
        ],
        2_400,
    )
}

fn rpgmaker_loop_midi() -> Vec<u8> {
    build_midi(
        vec![
            (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
            (0, vec![0xc0, PROGRAM as u8]),
            (0, vec![0xb0, 7, 100]),
            (0, vec![0x90, PITCH, VELOCITY]),
            (480, vec![0x80, PITCH, 0]),
            (480, vec![0xb0, 111, 127]),
            (960, vec![0x90, PITCH, 88]),
            (1_440, vec![0x80, PITCH, 0]),
        ],
        1_920,
    )
}

fn sample_loop_midi() -> Vec<u8> {
    build_midi(
        vec![
            (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
            (0, vec![0xc0, PROGRAM as u8]),
            (0, vec![0xb0, 7, 100]),
            (0, vec![0x90, PITCH, VELOCITY]),
            (480, vec![0x80, PITCH, 0]),
        ],
        960,
    )
}

fn build_midi(mut events: Vec<(u32, Vec<u8>)>, end_tick: u32) -> Vec<u8> {
    events.sort_by_key(|(tick, _)| *tick);
    let mut track = Vec::new();
    let mut previous_tick = 0;
    for (tick, event) in events {
        assert!(tick >= previous_tick, "MIDI event ticks are monotonic");
        write_variable_length(tick - previous_tick, &mut track);
        track.extend_from_slice(&event);
        previous_tick = tick;
    }
    assert!(
        end_tick >= previous_tick,
        "MIDI end tick follows all events"
    );
    write_variable_length(end_tick - previous_tick, &mut track);
    track.extend_from_slice(&[0xff, 0x2f, 0]);

    let mut midi = Vec::with_capacity(22 + track.len());
    midi.extend_from_slice(b"MThd");
    midi.extend_from_slice(&6_u32.to_be_bytes());
    midi.extend_from_slice(&0_u16.to_be_bytes());
    midi.extend_from_slice(&1_u16.to_be_bytes());
    midi.extend_from_slice(&480_u16.to_be_bytes());
    midi.extend_from_slice(b"MTrk");
    midi.extend_from_slice(&(track.len() as u32).to_be_bytes());
    midi.extend_from_slice(&track);
    midi
}

fn write_variable_length(mut value: u32, out: &mut Vec<u8>) {
    assert!(
        value <= 0x0fff_ffff,
        "MIDI variable-length quantity fits four bytes"
    );
    let mut bytes = [0_u8; 4];
    let mut index = 3;
    bytes[index] = (value & 0x7f) as u8;
    while {
        value >>= 7;
        value != 0
    } {
        index -= 1;
        bytes[index] = ((value & 0x7f) as u8) | 0x80;
    }
    out.extend_from_slice(&bytes[index..]);
}

fn build_sf2(initial_attenuation_centibels: i16) -> Vec<u8> {
    build_sf2_with_options(initial_attenuation_centibels, 64, 4_032, false, false)
}

fn build_invalid_sample_loop_sf2() -> Vec<u8> {
    build_sf2_with_options(0, 4_032, 64, true, false)
}

fn build_odd_smpl_sf2() -> Vec<u8> {
    build_sf2_with_options(0, 64, 4_032, false, true)
}

fn build_sf2_with_options(
    initial_attenuation_centibels: i16,
    sample_loop_start: i32,
    sample_loop_end: i32,
    looping_enabled: bool,
    odd_smpl_chunk: bool,
) -> Vec<u8> {
    let mut info = Vec::new();
    append_chunk(b"ifil", &[2, 0, 1, 0], &mut info);
    append_chunk(b"isng", b"EMU8000\0", &mut info);
    // SF2 INFO strings include their even-sized NUL padding in the chunk length.
    append_chunk(b"INAM", b"DEPS-BATCH-025\0\0", &mut info);
    let standard_sample_bytes = (SF2_SAMPLE_FRAMES + SF2_GUARD_FRAMES) * 2;

    let odd_smpl_extra_bytes = if odd_smpl_chunk { 1 } else { 0 };
    let mut sample_data = Vec::with_capacity(standard_sample_bytes + odd_smpl_extra_bytes);
    for _ in 0..64 {
        for _ in 0..32 {
            append_i16(8_192, &mut sample_data);
        }
        for _ in 0..32 {
            append_i16(-8_192, &mut sample_data);
        }
    }
    for _ in 0..SF2_GUARD_FRAMES {
        append_i16(0, &mut sample_data);
    }
    if odd_smpl_chunk {
        sample_data.push(0);
    }
    assert_eq!(
        sample_data.len(),
        standard_sample_bytes + odd_smpl_extra_bytes
    );

    let mut preset_headers = Vec::new();
    append_fixed_name(b"Loud Fixture", 20, &mut preset_headers);
    append_u16(34, &mut preset_headers);
    append_u16(0, &mut preset_headers);
    append_u16(0, &mut preset_headers);
    append_i32(0, &mut preset_headers);
    append_i32(0, &mut preset_headers);
    append_i32(0, &mut preset_headers);
    append_fixed_name(b"EOP", 20, &mut preset_headers);
    append_u16(0, &mut preset_headers);
    append_u16(0, &mut preset_headers);
    append_u16(2, &mut preset_headers);
    append_i32(0, &mut preset_headers);
    append_i32(0, &mut preset_headers);
    append_i32(0, &mut preset_headers);

    let mut preset_bags = Vec::new();
    for generator_index in [0, 1, 2] {
        append_u16(generator_index, &mut preset_bags);
        append_u16(0, &mut preset_bags);
    }
    let mut preset_generators = Vec::new();
    append_generator(48, initial_attenuation_centibels, &mut preset_generators);
    append_generator(41, 0, &mut preset_generators);
    append_generator(0, 0, &mut preset_generators);

    let mut instrument_headers = Vec::new();
    append_fixed_name(b"Fixture Instrument", 20, &mut instrument_headers);
    append_u16(0, &mut instrument_headers);
    append_fixed_name(b"EOI", 20, &mut instrument_headers);
    append_u16(1, &mut instrument_headers);

    let mut instrument_bags = Vec::new();
    append_u16(0, &mut instrument_bags);
    append_u16(0, &mut instrument_bags);
    append_u16(if looping_enabled { 2 } else { 1 }, &mut instrument_bags);
    append_u16(0, &mut instrument_bags);
    let mut instrument_generators = Vec::new();
    if looping_enabled {
        append_generator(54, 1, &mut instrument_generators);
    }
    // sampleID terminates a local instrument zone in SF2.
    append_generator(53, 0, &mut instrument_generators);
    append_generator(0, 0, &mut instrument_generators);

    let mut sample_headers = Vec::new();
    append_fixed_name(b"Fixture Sample", 20, &mut sample_headers);
    for value in [
        0,
        SF2_SAMPLE_FRAMES as i32,
        sample_loop_start,
        sample_loop_end,
        SAMPLE_RATE as i32,
    ] {
        append_i32(value, &mut sample_headers);
    }
    sample_headers.push(PITCH);
    sample_headers.push(0);
    append_u16(0, &mut sample_headers);
    append_u16(1, &mut sample_headers);
    append_fixed_name(b"EOS", 20, &mut sample_headers);
    let terminal_sample = (SF2_SAMPLE_FRAMES + SF2_GUARD_FRAMES) as i32;
    for value in [
        terminal_sample,
        terminal_sample,
        terminal_sample,
        terminal_sample,
        SAMPLE_RATE as i32,
    ] {
        append_i32(value, &mut sample_headers);
    }
    sample_headers.push(0);
    sample_headers.push(0);
    append_u16(0, &mut sample_headers);
    append_u16(1, &mut sample_headers);

    let mut pdta = Vec::new();
    append_chunk(b"phdr", &preset_headers, &mut pdta);
    append_chunk(b"pbag", &preset_bags, &mut pdta);
    append_chunk(b"pmod", &[0; 10], &mut pdta);
    append_chunk(b"pgen", &preset_generators, &mut pdta);
    append_chunk(b"inst", &instrument_headers, &mut pdta);
    append_chunk(b"ibag", &instrument_bags, &mut pdta);
    append_chunk(b"imod", &[0; 10], &mut pdta);
    append_chunk(b"igen", &instrument_generators, &mut pdta);
    append_chunk(b"shdr", &sample_headers, &mut pdta);

    let mut form = Vec::new();
    form.extend_from_slice(b"sfbk");
    form.extend_from_slice(&list_chunk(b"INFO", &info));
    let mut sdta = Vec::new();
    append_chunk(b"smpl", &sample_data, &mut sdta);
    form.extend_from_slice(&list_chunk(b"sdta", &sdta));
    form.extend_from_slice(&list_chunk(b"pdta", &pdta));
    chunk(b"RIFF", &form)
}

fn list_chunk(list_type: &[u8; 4], chunks: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + chunks.len());
    payload.extend_from_slice(list_type);
    payload.extend_from_slice(chunks);
    chunk(b"LIST", &payload)
}

fn append_chunk(id: &[u8; 4], payload: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&chunk(id, payload));
}

fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len() + (payload.len() % 2));
    out.extend_from_slice(id);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

fn append_fixed_name(name: &[u8], width: usize, out: &mut Vec<u8>) {
    assert!(name.len() <= width);
    out.extend_from_slice(name);
    out.resize(out.len() + width - name.len(), 0);
}

fn append_generator(operation: u16, amount: i16, out: &mut Vec<u8>) {
    append_u16(operation, out);
    append_u16(amount as u16, out);
}

fn append_u16(value: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_i16(value: i16, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_i32(value: i32, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn locked_rustysynth_version() -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let versions: Vec<_> = lock
        .split("[[package]]")
        .filter_map(|package| {
            let name = package.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("name = \"")
                    .and_then(|value| value.strip_suffix('\"'))
            });
            if name != Some("rustysynth") {
                return None;
            }
            package.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("version = \"")
                    .and_then(|value| value.strip_suffix('\"'))
            })
        })
        .collect();
    assert_eq!(
        versions.len(),
        1,
        "client Cargo.lock has one rustysynth entry"
    );
    versions[0].to_owned()
}

fn validate_baseline_identity(receipt: &Value, assets: &[InputAsset], fixtures: &Fixtures) {
    assert_eq!(
        receipt["schema"], "deps-batch-025-midi-pcm-differential-v2",
        "baseline receipt schema"
    );
    assert_eq!(receipt["sample_rate_hz"], json!(SAMPLE_RATE));
    assert_eq!(receipt["channels"], json!(CHANNELS));
    assert_eq!(
        receipt["target"],
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
    );
    assert_eq!(receipt["sample_format"], "f32-le-interleaved-stereo");
    assert_eq!(
        receipt["sound_font_selection"],
        json!({
            "method": "explicit RustyMidi::with_sound_font",
            "corpus_relative_path": "client/SCC1_Florestan.sf2",
            "sha256": assets[0].sha256,
            "cache_search_used": false,
            "working_directory_fallback_used": false,
        })
    );
    assert_eq!(
        receipt["effects"],
        json!({"reverb": false, "chorus": false})
    );
    assert_eq!(receipt["master_volume"], 0.5);
    let odd_smpl_case = &receipt["sf2_edge_cases"]["odd_smpl_chunk"];
    assert_eq!(odd_smpl_case["fixture"], fixtures.odd_smpl_font.name);
    assert_eq!(
        odd_smpl_case["declared_smpl_bytes"],
        json!((SF2_SAMPLE_FRAMES + SF2_GUARD_FRAMES) * 2 + 1)
    );
    assert!(odd_smpl_case["declared_size_is_odd"]
        .as_bool()
        .expect("odd-smpl size parity is present"));
    assert!(odd_smpl_case["isolated_subprocess"]
        .as_bool()
        .expect("odd-smpl subprocess status is present"));
    assert!(!odd_smpl_case["pcm_generated"]
        .as_bool()
        .expect("odd-smpl PCM status is present"));
    assert_eq!(odd_smpl_case["operator_acceptance"], "not-decided");
    assert!(odd_smpl_case["outcome"].is_object());
    let invalid_loop_case = &receipt["sf2_edge_cases"]["invalid_sample_loop"];
    assert_eq!(
        invalid_loop_case["fixture"],
        fixtures.invalid_sample_loop_font.name
    );
    assert_eq!(
        invalid_loop_case["instrument_generator"],
        json!({
            "operation": "sampleModes",
            "generator_id": 54,
            "value": 1,
            "mode": "Continuous",
        })
    );
    assert_eq!(invalid_loop_case["sample_loop_start"], 4_032);
    assert_eq!(invalid_loop_case["sample_loop_end"], 64);
    if invalid_loop_case["pcm_generated"] == Value::Bool(true) {
        assert_eq!(
            invalid_loop_case["outcome"]["production_load_status"],
            "accepted"
        );
        assert_eq!(
            invalid_loop_case["outcome"]["production_render_status"],
            "completed"
        );
    }
    assert_eq!(
        invalid_loop_case["pcm_generated"].as_bool(),
        Some(invalid_loop_case["outcome"]["render_capture"].is_object())
    );
    assert_eq!(invalid_loop_case["operator_acceptance"], "not-decided");
    assert!(invalid_loop_case["outcome"].is_object());
    let expected_assets: Vec<Value> = assets
        .iter()
        .map(|asset| {
            json!({
                "path": asset.path,
                "sha256": asset.sha256,
                "byte_len": asset.bytes.len(),
            })
        })
        .collect();
    let expected_fixtures: Vec<Value> = fixture_list(fixtures)
        .iter()
        .map(|fixture| {
            json!({
                "name": fixture.name,
                "sha256": fixture.sha256,
                "byte_len": fixture.bytes.len(),
            })
        })
        .collect();
    assert_eq!(receipt["asset_identities"], json!(expected_assets));
    assert_eq!(receipt["fixture_identities"], json!(expected_fixtures));
    assert_eq!(
        receipt["capture_stage"],
        "RustyMidi::render before mixer fade and audio device"
    );
}

fn compare_pcm(candidate: &[u8], baseline: &[u8]) -> Value {
    let candidate_well_formed = candidate.len().is_multiple_of(CHANNELS * BYTES_PER_SAMPLE);
    let baseline_well_formed = baseline.len().is_multiple_of(CHANNELS * BYTES_PER_SAMPLE);
    let candidate_samples = candidate.len() / BYTES_PER_SAMPLE;
    let baseline_samples = baseline.len() / BYTES_PER_SAMPLE;
    let comparable_samples = candidate_samples.min(baseline_samples);
    let mut first_changed = None;
    let mut changed_sample_count = 0;
    let mut max_abs_delta = 0.0_f32;
    let mut finite_comparison = true;

    for sample_index in 0..comparable_samples {
        let offset = sample_index * BYTES_PER_SAMPLE;
        let candidate_bytes: [u8; BYTES_PER_SAMPLE] = candidate[offset..offset + BYTES_PER_SAMPLE]
            .try_into()
            .expect("four-byte f32 sample");
        let baseline_bytes: [u8; BYTES_PER_SAMPLE] = baseline[offset..offset + BYTES_PER_SAMPLE]
            .try_into()
            .expect("four-byte f32 sample");
        let candidate_bits = u32::from_le_bytes(candidate_bytes);
        let baseline_bits = u32::from_le_bytes(baseline_bytes);
        let candidate_value = f32::from_bits(candidate_bits);
        let baseline_value = f32::from_bits(baseline_bits);
        finite_comparison &= candidate_value.is_finite() && baseline_value.is_finite();
        if candidate_bits != baseline_bits {
            changed_sample_count += 1;
            if first_changed.is_none() {
                first_changed = Some(json!({
                    "frame": sample_index / CHANNELS,
                    "channel_index": sample_index % CHANNELS,
                    "channel": if sample_index.is_multiple_of(CHANNELS) { "left" } else { "right" },
                    "candidate_f32_bits_le_hex": format!("{candidate_bits:08x}"),
                    "baseline_f32_bits_le_hex": format!("{baseline_bits:08x}"),
                }));
            }
        }
        if candidate_value.is_finite() && baseline_value.is_finite() {
            max_abs_delta = max_abs_delta.max((candidate_value - baseline_value).abs());
        }
    }

    let unmatched_samples = candidate_samples.abs_diff(baseline_samples);
    changed_sample_count += unmatched_samples;
    if unmatched_samples != 0 && first_changed.is_none() {
        let first_unmatched = comparable_samples;
        first_changed = Some(json!({
            "frame": first_unmatched / CHANNELS,
            "channel_index": first_unmatched % CHANNELS,
            "channel": if first_unmatched.is_multiple_of(CHANNELS) { "left" } else { "right" },
            "candidate_f32_bits_le_hex": Value::Null,
            "baseline_f32_bits_le_hex": Value::Null,
        }));
    }

    let frame_count_matches =
        candidate_samples == baseline_samples && candidate_well_formed && baseline_well_formed;
    let exact_bit_match = candidate == baseline;
    let status = if exact_bit_match {
        "exact-bit-match"
    } else {
        "difference-observed-operator-ruling-required"
    };
    json!({
        "status": status,
        "exact_bit_match": exact_bit_match,
        "candidate_byte_len": candidate.len(),
        "baseline_byte_len": baseline.len(),
        "candidate_frame_count": candidate_samples / CHANNELS,
        "baseline_frame_count": baseline_samples / CHANNELS,
        "frame_count_matches": frame_count_matches,
        "candidate_stereo_f32_file_well_formed": candidate_well_formed,
        "baseline_stereo_f32_file_well_formed": baseline_well_formed,
        "first_changed_sample": first_changed,
        "changed_sample_count": changed_sample_count,
        "max_abs_delta_over_finite_comparable_samples": max_abs_delta,
        "finite_comparison": finite_comparison,
        "unmatched_scalar_sample_count": unmatched_samples,
        "tolerance_applied": false,
        "operator_acceptance": "not-decided",
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
