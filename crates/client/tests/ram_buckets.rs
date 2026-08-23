// RAM bucket dump: prints the sizes of the big extra-client allocations so
// the orch can decide whether 4.6e (iface CoW) is needed. No production code.
use client::dash3d::Square;
use client::graphics::pix3d::{ModelScratch, Pix3DDraw};
use client::sound::jagfx::JagFX;

/// Task 3: a fresh `Pix3DDraw` must not own the 1500×512 depth table (or any
/// of the projection scratch) until the 3D render path runs `ensure()`.
#[test]
fn pix3d_default_has_no_depth_table() {
    let d = Pix3DDraw::default();
    assert!(d.model_scratch.tmp_depth_faces.is_empty());
}

#[test]
fn ram_bucket_sizes() {
    let square = std::mem::size_of::<Square>();
    let fill = 104 * 104 * square;
    println!("size_of Square={square} fill_base_level≈{fill}");
    let scratch = ModelScratch::default();
    println!(
        "ModelScratch tmp_depth_faces={} bytes",
        scratch.tmp_depth_faces.len() * 4
    );
    assert_eq!(scratch.tmp_depth_faces.len(), 1500 * 512);
    let _ = JagFX::default();
    println!(
        "JagFX WAVE_BYTES={} TONE_BUF_elems=220500",
        22050 * 20
    );
}

#[test]
fn ram_iface_template_estimate() {
    // The e2e default pack path (client-play `default_cache_dir`) when
    // neither CACHE_DIR nor CLIENT_CACHE is set.
    let dir = std::env::var("CACHE_DIR")
        .or_else(|_| std::env::var("CLIENT_CACHE"))
        .unwrap_or_else(|_| {
            match std::env::var("HOME") {
                Ok(home) => format!("{home}/experiments/Server/engine/data/pack/client"),
                Err(_) => "experiments/Server/engine/data/pack/client".into(),
            }
        });
    let path = format!("{dir}/interface");
    let Ok(bytes) = std::fs::read(&path) else {
        println!("no interface pack at {path}");
        return;
    };
    let ifaces = client::config::IfType::unpack(&client::io::JagFile::new(bytes));
    let some = ifaces.iter().filter(|s| s.is_some()).count();
    let slot = std::mem::size_of::<client::config::IfType>();
    println!(
        "ifaces len={} some={} size_of IfType={} slots≈{}",
        ifaces.len(),
        some,
        slot,
        ifaces.len() * slot
    );
}
