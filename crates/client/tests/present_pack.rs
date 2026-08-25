//! Present targets (task 6): `pack_rgb` maps a PixMap `i32` pixel (low 24
//! bits 0xRRGGBB) onto the softbuffer `u32` layout the window presents, and
//! `present_target_enum_dispatch` drives the two `PresentTarget` paths
//! (`Window` blit, `Textures` host handoff) through one trait object. Kept
//! always-on so it compiles without the `window` feature.

use client::client::present::{PresentTarget, TexturesTarget, WindowTarget};
use client::graphics::PixMap;
use client::render::backend::FrameOutput;

#[test]
fn pack_low24() {
    assert_eq!(client::client::present::pack_rgb(0x00AABBCC), 0x00AABBCC);
    assert_eq!(client::client::present::pack_rgb(-1), 0x00FFFFFF); // low 24 of 0xFFFFFFFF
}

/// The present seam is a trait object, not a softbuffer call: a `Window`
/// target and a `Textures` target both accept a `FrameOutput` through the
/// same `&mut dyn PresentTarget`. The window path blits the PixMap (here
/// surface-less: frames are counted and dropped, the same degrade softbuffer
/// applies to a lost surface); the textures path retains the handed frame
/// for the host.
#[test]
fn present_target_enum_dispatch() {
    let mut window = WindowTarget::headless();
    let mut textures = TexturesTarget::new();
    let mut frame = PixMap::new(2, 2);
    frame.pixels[0] = 0x00AABBCC;
    frame.pixels[3] = -1;

    // Both paths driven through one trait-object slice; the call site
    // mentions neither softbuffer nor a concrete target.
    let mut targets: Vec<&mut dyn PresentTarget> = vec![&mut window, &mut textures];
    for target in &mut targets {
        target.present(FrameOutput::PixMap(frame.clone()));
    }

    // Textures path: the host received the handed frame (uploading/blitting
    // it is the host's job — the seam hands the output off, it does not
    // upload).
    match textures.take_frame() {
        Some(FrameOutput::PixMap(host_frame)) => {
            assert_eq!(
                (host_frame.width, host_frame.height),
                (frame.width, frame.height)
            );
            assert_eq!(host_frame.pixels, frame.pixels);
        }
        _ => panic!("Textures target must retain the handed frame for the host"),
    }
    // Window path: the frame reached the target (dispatch) and was dropped
    // by the surface-less blit.
    assert_eq!(window.frames(), 1);
}
