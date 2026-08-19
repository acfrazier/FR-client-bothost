//! Present RGB pack: `pack_rgb` maps a PixMap `i32` pixel (low 24 bits
//! 0xRRGGBB) onto the softbuffer `u32` layout the window presents. Kept
//! always-on so it compiles without the `window` feature.

#[test]
fn pack_low24() {
    assert_eq!(client::client::present::pack_rgb(0x00AABBCC), 0x00AABBCC);
    assert_eq!(client::client::present::pack_rgb(-1), 0x00FFFFFF); // low 24 of 0xFFFFFFFF
}
