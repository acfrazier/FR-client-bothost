use client::graphics::{Colour, Pix2D, Pix32, Pix3D, Pix3DDraw, Pix8, PixFont, PixMap};
use client::io::JagFile;
use client::util::JavaRandom;
use std::io::Write;

fn g2(out: &mut Vec<u8>, v: i32) {
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn g3(out: &mut Vec<u8>, v: i32) {
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn bz2(data: &[u8]) -> Vec<u8> {
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(data).unwrap();
    let out = enc.finish().unwrap();
    // Jagex streams omit the "BZh<level>" file header (see io/bzip2.rs)
    assert!(out.starts_with(b"BZh"));
    out[4..].to_vec()
}

/// Jag container in the equal-size (per-file bzip2) layout: g3 size, g3 size,
/// g2 file count, per-file g4 hash + g3 unpacked + g3 packed, then the packed
/// bytes of each file.
fn jag(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2(d)).collect();
    let data_len: usize = packed.iter().map(|d| d.len()).sum();
    let total = (8 + 10 * files.len() + data_len) as i32;
    g3(&mut out, total);
    g3(&mut out, total);
    out.push((files.len() >> 8) as u8);
    out.push(files.len() as u8);
    for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
        out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        g3(&mut out, data.len() as i32);
        g3(&mut out, packed_data.len() as i32);
    }
    for d in &packed {
        out.extend_from_slice(d);
    }
    out
}

#[test]
fn pixmap_fill_rect() {
    let mut map = PixMap::new(4, 4);
    map.fill(0x00ff00);
    assert_eq!(map.pixels.len(), 16);
    assert!(map.pixels.iter().all(|&p| p == 0x00ff00));
}

#[test]
fn pix2d_fill_rect_honours_clipping() {
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.set_clipping(1, 1, 3, 3);
        surface.fill_rect(0, 0, 4, 4, 0xff0000);
    }
    let expected = [
        0, 0, 0, 0, //
        0, 0xff0000, 0xff0000, 0, //
        0, 0xff0000, 0xff0000, 0, //
        0, 0, 0, 0,
    ];
    assert_eq!(map.pixels, expected);
}

#[test]
fn pix2d_fill_rect_trans_mixes_channels() {
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect_trans(0, 0, 4, 4, 0xff0000, 128);
    }
    assert!(map.pixels.iter().all(|&p| p == 0x7f0000));

    let mut map2 = PixMap::new(2, 2);
    {
        let mut s = Pix2D::with_pixels(&mut map2.pixels, map2.width, map2.height);
        s.fill_rect_trans(0, 0, 2, 2, 0xffffff, 64);
    }
    assert!(map2.pixels.iter().all(|&p| p == 0x3f3f3f));
}

#[test]
fn pix2d_draw_rect_outlines_only() {
    let mut map = PixMap::new(6, 6);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.cls();
        surface.draw_rect(1, 1, 4, 4, 0xff0000);
    }
    assert_eq!(map.pixels[7], 0xff0000); // (1,1)
    assert_eq!(map.pixels[10], 0xff0000); // (4,1)
    assert_eq!(map.pixels[25], 0xff0000); // (1,4)
    assert_eq!(map.pixels[28], 0xff0000); // (4,4)
    assert_eq!(map.pixels[14], 0); // (2,2) interior
    assert_eq!(map.pixels[21], 0); // (3,3) interior
}

#[test]
fn pix2d_hline_vline() {
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.hline(0, 0, 4, 0xff0000);
        surface.vline(3, 0, 4, 0x00ff00);
    }
    assert_eq!(map.pixels[0], 0xff0000);
    assert_eq!(map.pixels[3], 0x00ff00);
    assert_eq!(map.pixels[7], 0x00ff00);
    assert_eq!(map.pixels[11], 0x00ff00);
    assert_eq!(map.pixels[5], 0);
}

#[test]
fn pix2d_cls_clears_all() {
    let mut map = PixMap::new(3, 3);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect(0, 0, 3, 3, 0xffffff);
        surface.cls();
    }
    assert!(map.pixels.iter().all(|&p| p == 0));
}

#[test]
fn pix2d_fill_circle_opaque() {
    let mut map = PixMap::new(5, 5);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_circle(2, 2, 1, 0xff0000, 256);
    }
    let expected = [
        0, 0, 0, 0, 0, //
        0, 0, 0xff0000, 0, 0, //
        0, 0xff0000, 0xff0000, 0xff0000, 0, //
        0, 0, 0xff0000, 0, 0, //
        0, 0, 0, 0, 0,
    ];
    assert_eq!(map.pixels, expected);
}

#[test]
fn pix3d_div_tables() {
    assert_eq!(Pix3D::div_table()[0], 0);
    assert_eq!(Pix3D::div_table()[1], 32768);
    assert_eq!(Pix3D::div_table()[2], 16384);
    assert_eq!(Pix3D::div_table()[511], 64);
    assert_eq!(Pix3D::div_table2()[0], 0);
    assert_eq!(Pix3D::div_table2()[1], 65536);
    assert_eq!(Pix3D::div_table2()[1024], 64);
    assert_eq!(Pix3D::div_table2()[2047], 32);
}

#[test]
fn pix3d_sin_cos_tables_match_formula() {
    let sin = Pix3D::sin_table();
    let cos = Pix3D::cos_table();
    assert_eq!(sin.len(), 2048);
    assert_eq!(cos.len(), 2048);
    for i in 0..2048 {
        let expected_sin = (f64::sin(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
        let expected_cos = (f64::cos(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
        assert_eq!(sin[i], expected_sin, "sin_table[{i}]");
        assert_eq!(cos[i], expected_cos, "cos_table[{i}]");
    }
    assert_eq!(sin[0], 0);
    assert_eq!(cos[0], 65536);
    assert_eq!(sin[256], 46340);
    assert_eq!(sin[1024], 0);
}

#[test]
fn pix3d_colour_table_structure() {
    Pix3D::init_colour_table(0.6);
    let table = Pix3D::colour_table();
    assert_eq!(table.len(), 65536);
    assert_eq!(table[0], 0);
    assert!(table.iter().all(|&v| (0..=0xffffff).contains(&v)));
    Pix3D::init_colour_table(0.6);
    assert_eq!(table, Pix3D::colour_table());
}

#[test]
fn colour_constants_match_274() {
    assert_eq!(Colour::RED, 0xff0000);
    assert_eq!(Colour::GREEN, 0xff00);
    assert_eq!(Colour::BLUE, 0xff);
    assert_eq!(Colour::YELLOW, 0xffff00);
    assert_eq!(Colour::CYAN, 0xffff);
    assert_eq!(Colour::MAGENTA, 0xff00ff);
    assert_eq!(Colour::WHITE, 0xffffff);
    assert_eq!(Colour::BLACK, 0);
    assert_eq!(Colour::LIGHTRED, 0xff9040);
    assert_eq!(Colour::DARKRED, 0x800000);
    assert_eq!(Colour::DARKBLUE, 0x80);
    assert_eq!(Colour::ORANGE1, 0xffb000);
    assert_eq!(Colour::ORANGE2, 0xff7000);
    assert_eq!(Colour::ORANGE3, 0xff3000);
    assert_eq!(Colour::GREEN1, 0xc0ff00);
    assert_eq!(Colour::GREEN2, 0x80ff00);
    assert_eq!(Colour::GREEN3, 0x40ff00);
}

#[test]
fn java_random_known_sequence() {
    let mut rng = JavaRandom::new(1337);
    assert_eq!(rng.next_int(), -1460590454);
    assert_eq!(rng.next_int(), 747279288);
    assert_eq!(rng.next_int(), -1334692577);
    assert_eq!(rng.next_int(), -539670452);
    assert_eq!(rng.next_int(), -501340078);

    // well-known Java util.Random(1) first nextInt
    let mut rng = JavaRandom::new(1);
    assert_eq!(rng.next_int(), -1155869325);
}

#[test]
fn java_random_next_int_bound() {
    let mut rng = JavaRandom::new(1337);
    let values: Vec<i32> = (0..5).map(|_| rng.next_int_bound(100)).collect();
    assert_eq!(values, [21, 44, 59, 22, 9]);

    let mut rng = JavaRandom::new(1337);
    let pow2: Vec<i32> = (0..5).map(|_| rng.next_int_bound(64)).collect();
    assert_eq!(pow2, [42, 11, 44, 55, 56]);
}

fn pix8_archive() -> Vec<u8> {
    let mut index = Vec::new();
    g2(&mut index, 2); // owi
    g2(&mut index, 2); // ohi
    index.push(4); // bpalCount
    g3(&mut index, 0x00ff00);
    g3(&mut index, 0xff0000);
    g3(&mut index, 0x0000ff);
    index.push(0); // xof
    index.push(0); // yof
    g2(&mut index, 2); // wi
    g2(&mut index, 2); // hi
    index.push(0); // encoding 0: row-major
    let mut dat = Vec::new();
    g2(&mut dat, 0); // sprite 0 header starts at index.dat offset 0
    dat.extend_from_slice(&[1, 2, 3, 0]);
    jag(&[("index.dat", &index), ("8.dat", &dat)])
}

#[test]
fn pix8_depack_and_plot_sprite() {
    let j = JagFile::new(pix8_archive());
    let sprite = Pix8::depack(&j, "8", 0).unwrap();
    assert_eq!(sprite.owi, 2);
    assert_eq!(sprite.ohi, 2);
    assert_eq!(sprite.wi, 2);
    assert_eq!(sprite.hi, 2);
    assert_eq!(sprite.data, vec![1i8, 2, 3, 0]);
    assert_eq!(sprite.bpal, vec![0, 0x00ff00, 0xff0000, 0x0000ff]);

    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        sprite.plot_sprite(&mut surface, 1, 1);
    }
    assert_eq!(map.pixels[5], 0x00ff00);
    assert_eq!(map.pixels[6], 0xff0000);
    assert_eq!(map.pixels[9], 0x0000ff);
    assert_eq!(map.pixels[10], 0); // palette index 0 is transparent
}

#[test]
fn pix8_trim_restores_original_size() {
    let mut s = Pix8::new(1, 1, vec![0, 0x00ff00]);
    s.owi = 3;
    s.ohi = 3;
    s.xof = 1;
    s.yof = 1;
    s.data[0] = 1;
    s.trim();
    assert_eq!(s.wi, 3);
    assert_eq!(s.hi, 3);
    assert_eq!(s.xof, 0);
    assert_eq!(s.yof, 0);
    assert_eq!(s.data.len(), 9);
    assert_eq!(s.data[4], 1);
    assert_eq!(s.data[0], 0);
}

#[test]
fn pix8_halve_size() {
    let mut s = Pix8::new(4, 4, vec![0, 1]);
    // dst = (x >> 1) + (y >> 1) * 2; last writer per 2x2 block wins
    s.data[5] = 1;
    s.data[7] = 1;
    s.data[13] = 1;
    s.data[15] = 1;
    s.halve_size();
    assert_eq!(s.owi, 2);
    assert_eq!(s.ohi, 2);
    assert_eq!(s.wi, 2);
    assert_eq!(s.hi, 2);
    assert_eq!(s.data, vec![1i8, 1, 1, 1]);
}

#[test]
fn pix8_flips() {
    let mut s = Pix8::new(2, 2, vec![0, 1]);
    s.data = vec![1, 2, 3, 4];
    s.hflip();
    assert_eq!(s.data, vec![2, 1, 4, 3]);

    let mut s = Pix8::new(2, 2, vec![0, 1]);
    s.data = vec![1, 2, 3, 4];
    s.vflip();
    assert_eq!(s.data, vec![3, 4, 1, 2]);
}

#[test]
fn pix8_scale_plot_sprite() {
    let mut sprite = Pix8::new(1, 1, vec![0, 0xff0000]);
    sprite.data[0] = 1;
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        sprite.scale_plot_sprite(&mut surface, 0, 0, 2, 2);
    }
    assert_eq!(map.pixels[0], 0xff0000);
    assert_eq!(map.pixels[1], 0xff0000);
    assert_eq!(map.pixels[4], 0xff0000);
    assert_eq!(map.pixels[5], 0xff0000);
    assert_eq!(map.pixels[2], 0);
}

fn pix32_archive() -> Vec<u8> {
    let mut index = Vec::new();
    g2(&mut index, 2); // owi
    g2(&mut index, 2); // ohi
    index.push(4); // bpalCount
    g3(&mut index, 0x00ff00);
    g3(&mut index, 0x000000); // zero palette entries map to 1
    g3(&mut index, 0x0000ff);
    index.push(0); // xof
    index.push(0); // yof
    g2(&mut index, 2); // wi
    g2(&mut index, 2); // hi
    index.push(0); // encoding 0: row-major
    let mut dat = Vec::new();
    g2(&mut dat, 0);
    dat.extend_from_slice(&[1, 2, 3, 0]);
    jag(&[("index.dat", &index), ("12.dat", &dat)])
}

#[test]
fn pix32_depack_zero_palette_becomes_one() {
    let j = JagFile::new(pix32_archive());
    let sprite = Pix32::depack(&j, "12", 0).unwrap();
    assert_eq!(sprite.data, vec![0x00ff00, 1, 0x0000ff, 0]);
}

#[test]
fn pix32_plot_and_quick_plot() {
    let mut sprite = Pix32::new(2, 2);
    sprite.data = vec![0x00ff00, 0xff0000, 0x0000ff, 0];

    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect(0, 0, 4, 4, 0xffffff);
        sprite.plot_sprite(&mut surface, 1, 1);
    }
    assert_eq!(map.pixels[5], 0x00ff00);
    assert_eq!(map.pixels[6], 0xff0000);
    assert_eq!(map.pixels[9], 0x0000ff);
    assert_eq!(map.pixels[10], 0xffffff); // zero pixels are transparent

    let mut map2 = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map2.pixels, map2.width, map2.height);
        surface.fill_rect(0, 0, 4, 4, 0xffffff);
        sprite.quick_plot_sprite(&mut surface, 1, 1);
    }
    assert_eq!(map2.pixels[10], 0); // quick plot writes zero pixels verbatim
}

#[test]
fn pix32_trans_plot_sprite_blends() {
    let mut sprite = Pix32::new(2, 2);
    sprite.data = vec![0xff0000; 4];
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect(0, 0, 4, 4, 0xffffff);
        sprite.trans_plot_sprite(&mut surface, 0, 0, 128);
    }
    // TS int32 wrap sign-extends the top byte; low 24 bits are 0xff7f7f
    assert_eq!(map.pixels[0], 0xffff_7f7fu32 as i32);
    assert_eq!(map.pixels[1], 0xffff_7f7fu32 as i32);
    assert_eq!(map.pixels[4], 0xffff_7f7fu32 as i32);
    assert_eq!(map.pixels[5], 0xffff_7f7fu32 as i32);
    assert_eq!(map.pixels[3], 0xffffff); // outside the 2x2 sprite
}

#[test]
fn pix32_rotate_plot_sprite_identity() {
    let mut sprite = Pix32::new(2, 2);
    sprite.data = vec![0xff0000, 0x00ff00, 0x0000ff, 0xff00ff];
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        // anchor (1,1) centres the 2x2 sprite at (0,0) for zoom 256
        sprite.rotate_plot_sprite(&mut surface, 0, 0, 2, 2, 1, 1, 0.0, 256);
    }
    assert_eq!(map.pixels[0], 0xff0000);
    assert_eq!(map.pixels[1], 0x00ff00);
    assert_eq!(map.pixels[4], 0x0000ff);
    assert_eq!(map.pixels[5], 0xff00ff);
}

#[test]
fn pix32_scanline_plot_sprite_mask() {
    let mut sprite = Pix32::new(2, 2);
    sprite.data = vec![0xff0000, 0x00ff00, 0x0000ff, 0xff00ff];
    // mask is indexed by the destination offset; 4x4 covers the 4x4 surface
    let mut mask = Pix8::new(4, 4, vec![0]);
    mask.data[4] = 1; // blocks (0,1)
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect(0, 0, 4, 4, 0xffffff);
        sprite.scanline_plot_sprite(&mut surface, &mask, 0, 0);
    }
    assert_eq!(map.pixels[0], 0xff0000);
    assert_eq!(map.pixels[1], 0x00ff00);
    assert_eq!(map.pixels[4], 0xffffff); // masked off
    assert_eq!(map.pixels[5], 0xff00ff);
}

#[test]
fn pix32_trim_and_flip() {
    let mut s = Pix32::new(1, 1);
    s.data[0] = 0xff0000;
    s.owi = 3;
    s.ohi = 3;
    s.xof = 1;
    s.yof = 1;
    s.trim();
    assert_eq!(s.wi, 3);
    assert_eq!(s.hi, 3);
    assert_eq!(s.data.len(), 9);
    assert_eq!(s.data[4], 0xff0000);

    let mut f = Pix32::new(2, 2);
    f.data = vec![1, 2, 3, 4];
    f.hflip();
    assert_eq!(f.data, vec![2, 1, 4, 3]);
}

fn font_archive() -> Vec<u8> {
    let mut dat = Vec::new();
    g2(&mut dat, 0); // idx.pos = dat.g2() + 4
    let mut index = vec![0u8; 4];
    index.push(0); // palette count
    for _ in 0..256 {
        index.push(0); // charOffsetX
        index.push(0); // charOffsetY
        g2(&mut index, 1); // charMaskWidth
        g2(&mut index, 1); // charMaskHeight
        index.push(0); // pixel order
    }
    dat.extend([0u8; 256]); // one mask byte per char
    jag(&[("index.dat", &index), ("p11_full.dat", &dat)])
}

#[test]
fn pixfont_depack_from_jag() {
    let j = JagFile::new(font_archive());
    let font = PixFont::depack(&j, "p11_full", true).unwrap();
    assert_eq!(font.height, 1);
    assert_eq!(font.char_advance[65], 1);
    assert_eq!(font.char_advance[32], font.char_advance[73]); // quill space
    assert_eq!(font.string_wid(Some("hi")), 2);
    assert_eq!(font.string_wid(None), 0);
}

#[test]
fn pixfont_draw_string_plots_mask() {
    let mut font = PixFont::new();
    font.char_mask[65] = vec![1, 1, 1, 1];
    font.char_mask_width[65] = 2;
    font.char_mask_height[65] = 2;
    font.char_advance[65] = 4;
    font.height = 2;
    assert_eq!(font.string_wid(Some("A")), 4);
    assert_eq!(font.string_wid(Some("AA")), 8);

    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        font.draw_string(&mut surface, Some("A"), 0, 2, 0xff0000);
    }
    assert_eq!(map.pixels[0], 0xff0000);
    assert_eq!(map.pixels[1], 0xff0000);
    assert_eq!(map.pixels[4], 0xff0000);
    assert_eq!(map.pixels[5], 0xff0000);
    assert_eq!(map.pixels[2], 0);
}

#[test]
fn pixfont_draw_string_tag_colour() {
    let mut font = PixFont::new();
    font.char_mask[104] = vec![1]; // 'h'
    font.char_mask_width[104] = 1;
    font.char_mask_height[104] = 1;
    font.char_advance[104] = 2;
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        font.draw_string_tag(&mut surface, "@red@h", 0, 0, 0xffffff, false);
    }
    assert_eq!(map.pixels[0], Colour::RED);
}

#[test]
fn pixfont_anti_macro_plot_trans_blends() {
    let mut font = PixFont::new();
    font.char_mask[104] = vec![1]; // 'h'
    font.char_mask_width[104] = 1;
    font.char_mask_height[104] = 1;
    font.char_advance[104] = 2;
    let mut map = PixMap::new(4, 4);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        surface.fill_rect(0, 0, 4, 4, 0xffffff);
        // seed 42 → first nextInt = -1170105035 → alpha = (… & 0x1f) + 192 = 213;
        // shadow is black @ alpha 192 at (+1, +1). Expected values computed
        // from the TS PixFont.plotTrans expression with int32 wrapping
        // (per-term masks, separate shifts, then add — unlike Pix32.tranSprite,
        // whose mask-after-sum grouping genuinely differs in the TS source).
        font.draw_string_anti_macro(&mut surface, "h", 0, 0, 0xff0000, true, 42);
    }
    assert_eq!(map.pixels[0], 0xfffe_2a2au32 as i32); // main glyph blend
    assert_eq!(map.pixels[5], 0x3f3f3f); // shadow at (1,1)
    assert_eq!(map.pixels[1], 0xffffff); // untouched
    assert_eq!(map.pixels[4], 0xffffff); // untouched
}

#[test]
fn pixfont_update_state_tags() {
    let mut font = PixFont::new();
    assert_eq!(font.update_state("red"), Colour::RED);
    assert_eq!(font.update_state("gre"), Colour::GREEN);
    assert_eq!(font.update_state("blu"), Colour::BLUE);
    assert_eq!(font.update_state("yel"), Colour::YELLOW);
    assert_eq!(font.update_state("cya"), Colour::CYAN);
    assert_eq!(font.update_state("mag"), Colour::MAGENTA);
    assert_eq!(font.update_state("whi"), Colour::WHITE);
    assert_eq!(font.update_state("bla"), Colour::BLACK);
    assert_eq!(font.update_state("lre"), Colour::LIGHTRED);
    assert_eq!(font.update_state("dre"), Colour::DARKRED);
    assert_eq!(font.update_state("dbl"), Colour::DARKBLUE);
    assert_eq!(font.update_state("or1"), Colour::ORANGE1);
    assert_eq!(font.update_state("or2"), Colour::ORANGE2);
    assert_eq!(font.update_state("or3"), Colour::ORANGE3);
    assert_eq!(font.update_state("gr1"), Colour::GREEN1);
    assert_eq!(font.update_state("gr2"), Colour::GREEN2);
    assert_eq!(font.update_state("gr3"), Colour::GREEN3);
    assert_eq!(font.update_state("str"), -1);
    assert_eq!(font.update_state("nope"), -1);
    // `@str@` strikeout is per-call scratch (the shared fonts hold no
    // per-call state); `draw_string_tag` handles it locally.
}

#[test]
fn pixfont_centre_and_right_string() {
    let mut font = PixFont::new();
    font.char_mask[65] = vec![1, 1, 1, 1];
    font.char_mask_width[65] = 2;
    font.char_mask_height[65] = 2;
    font.char_advance[65] = 4;

    let mut map = PixMap::new(8, 8);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        font.centre_string(&mut surface, Some("A"), 2, 0, 0xff0000);
        // stringWid("A") = 4 → x = 2 - 2 = 0
    }
    assert_eq!(map.pixels[0], 0xff0000);

    let mut map2 = PixMap::new(8, 8);
    {
        let mut surface = Pix2D::with_pixels(&mut map2.pixels, map2.width, map2.height);
        font.draw_string_right(&mut surface, "A", 4, 0, 0xff0000, false);
        // x - stringWid = 0
    }
    assert_eq!(map2.pixels[0], 0xff0000);
}

#[test]
fn pix3d_draw_default_state() {
    let d = Pix3DDraw::default();
    assert!(d.scanline.is_empty());
    assert_eq!(d.origin_x, 0);
    assert_eq!(d.origin_y, 0);
    assert_eq!(d.trans, 0);
    assert_eq!(d.cycle, 0);
    assert!(!d.hclip);
    assert!(d.low_detail);
    assert!(!d.low_mem);
    assert_eq!(d.num_textures, 0);
    assert!(d.texel_pool.is_none());
}

#[test]
fn pix3d_set_clipping_sets_origin_and_scanline_len() {
    let mut d = Pix3DDraw::default();
    d.set_clipping(512, 334);
    assert_eq!(d.origin_x, 256);
    assert_eq!(d.origin_y, 167);
    assert_eq!(d.scanline.len(), 334);
    assert_eq!(d.scanline[1], 512);
}

#[test]
fn pix3d_flat_triangle_fills_pixmap() {
    let mut d = Pix3DDraw::default();
    let mut map = PixMap::new(5, 5);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        d.set_render_clipping(&surface);
        d.flat_triangle(&mut surface, 0, 4, 0, 0, 0, 4, 0xff0000);
    }
    let expected = [
        0xff0000, 0xff0000, 0xff0000, 0xff0000, 0, //
        0xff0000, 0xff0000, 0xff0000, 0, 0, //
        0xff0000, 0xff0000, 0, 0, 0, //
        0xff0000, 0, 0, 0, 0, //
        0, 0, 0, 0, 0,
    ];
    assert_eq!(map.pixels, expected);
}

#[test]
fn pix3d_gouraud_triangle_constant_shade() {
    Pix3D::init_colour_table(0.6);
    let mut d = Pix3DDraw::default();
    let mut map = PixMap::new(5, 5);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        d.set_render_clipping(&surface);
        d.gouraud_triangle(&mut surface, 0, 4, 0, 0, 0, 4, 256, 256, 256);
    }
    let rgb = Pix3D::colour_table()[256];
    // (0,0)-(4,0)-(0,4) upper-left triangle, constant shade → table[256]
    assert_eq!(map.pixels[0], rgb);
    assert_eq!(map.pixels[3], rgb);
    assert_eq!(map.pixels[7], rgb);
    assert_eq!(map.pixels[10], rgb);
    assert_eq!(map.pixels[15], rgb);
    assert_eq!(map.pixels[4], 0); // outside the triangle
    assert_eq!(map.pixels[24], 0);
}

/// GPU overlay coverage is attached via `coverage_guard`. TYPE_MODEL
/// (`obj_render` → gouraud/flat/texture rasters) must mark the same
/// pixels it writes, or the cube random-event 3D is a hole in the scene
/// window (and a wgpu validation/composite miss).
#[test]
fn pix3d_raster_marks_gpu_overlay_coverage() {
    Pix3D::init_colour_table(0.6);
    let mut d = Pix3DDraw::default();
    let mut map = PixMap::new(5, 5);
    let mut coverage = vec![0u8; 25];
    {
        let _g = client::graphics::pix2d::coverage_guard(&mut coverage, 5, 5);
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        d.set_render_clipping(&surface);
        d.gouraud_triangle(&mut surface, 0, 4, 0, 0, 0, 4, 256, 256, 256);
    }
    assert_eq!(
        coverage[0], 255,
        "written TYPE_MODEL pixel must be opaque overlay"
    );
    assert_eq!(coverage[4], 0, "unwritten pixel stays a scene hole");
}

#[test]
fn pix3d_flat_triangle_off_screen_writes_are_noops() {
    let mut d = Pix3DDraw::default();
    let mut map = PixMap::new(5, 5);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        d.set_render_clipping(&surface);
        // hclip=false and every vertex past an edge: the spans get negative
        // `off` (left) or past-the-buffer `off` (right). TS typed-array
        // writes are ignored there; the guarded write must not panic.
        d.flat_triangle(&mut surface, -40, -36, -40, 0, 0, 4, 0xff0000);
        d.flat_triangle(&mut surface, 30, 34, 30, 0, 0, 4, 0x00ff00);
    }
    assert!(map.pixels.iter().all(|&p| p == 0));
}

#[test]
fn pix3d_gouraud_triangle_off_screen_writes_are_noops() {
    Pix3D::init_colour_table(0.6);
    let mut d = Pix3DDraw::default();
    let mut map = PixMap::new(5, 5);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        d.set_render_clipping(&surface);
        d.gouraud_triangle(&mut surface, -40, -36, -40, 0, 0, 4, 256, 256, 256);
    }
    assert!(map.pixels.iter().all(|&p| p == 0));
}

fn pix8_texture_archive() -> Vec<u8> {
    let mut index = Vec::new();
    g2(&mut index, 1); // owi
    g2(&mut index, 1); // ohi
    index.push(2); // bpalCount
    g3(&mut index, 0x408040);
    index.push(0); // xof
    index.push(0); // yof
    g2(&mut index, 1); // wi
    g2(&mut index, 1); // hi
    index.push(0); // encoding 0: row-major
    let mut dat = Vec::new();
    g2(&mut dat, 0); // sprite 0 header starts at index.dat offset 0
    dat.push(1); // palette index 1
    jag(&[("index.dat", &index), ("0.dat", &dat)])
}

#[test]
fn pix3d_texture_pool_unpack_and_average() {
    let mut d = Pix3DDraw::default();
    d.init_pool(2);
    assert_eq!(d.pool_size, 2);
    assert_eq!(d.texel_pool.as_ref().unwrap().len(), 2);
    assert_eq!(d.texel_pool.as_ref().unwrap()[0].len(), 65536);

    d.unpack_textures(&JagFile::new(pix8_texture_archive()));
    assert_eq!(d.num_textures, 1);
    assert!(d.textures[0].is_some());

    d.init_texture_palettes(0.6);
    let pal = d.tex_pal[0].as_ref().unwrap();
    assert_eq!(pal.len(), 2);
    assert_eq!(pal[0], 0); // palette 0 is transparent
    assert_eq!(pal[1], 0x6fa86f); // gamma-corrected 0x408040 @ 0.6

    assert_eq!(d.get_texture_average(0), 0x1d351d);
    assert_eq!(d.get_texture_average(0), 0x1d351d); // cached
    assert_eq!(d.get_texture_average(1), 0); // missing texture
}
