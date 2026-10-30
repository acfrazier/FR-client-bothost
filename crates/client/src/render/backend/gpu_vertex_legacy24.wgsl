// Independent baseline from `git show 23ba29e2914a382ed69e5d73a668f9687b051c22:crates/client/src/render/backend/gpu.rs`.
// Its matching vertex input is 24 bytes: Float32x3, Uint32, Uint32, Uint32 at byte offsets 0, 12, 16, and 20.

const NEAR: f32 = 50.0;
const SCALE_X: f32 = 2.0;
const SCALE_Y: f32 = -512.0 / 167.0;

@group(0) @binding(0) var model_atlas: texture_2d_array<f32>;
@group(0) @binding(1) var model_sampler: sampler;

struct SceneUniforms {
    brightness: f32,
};
@group(1) @binding(0) var<uniform> scene: SceneUniforms;

// Six scalars preserve the CPU's 24-byte stride. vec3<f32> would align the
// record to 16 bytes and incorrectly advance storage reads by 32 bytes.
struct SceneVertex {
    x: f32,
    y: f32,
    z: f32,
    abhsl: u32,
    uv_tex: u32,
    v: u32,
};
@group(1) @binding(1) var<storage, read> vertices: array<SceneVertex>;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) abhsl: u32,
    @location(2) uv_tex: u32,
    @location(3) v: u32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(2) u: f32,
    @location(3) v: f32,
    @location(4) @interpolate(flat) tex_id: u32,
    @location(5) alpha: f32,
    @location(6) @interpolate(flat) shade_a: vec3<i32>,
    @location(7) @interpolate(flat) shade_b: vec3<i32>,
    @location(8) @interpolate(flat) shade_c: vec3<i32>,
    @location(9) @interpolate(flat) shade_ab: vec2<i32>,
    @location(10) @interpolate(flat) shade_bc: vec2<i32>,
    @location(11) @interpolate(flat) shade_ac: vec2<i32>,
    @location(12) @interpolate(flat) hclip: u32,
};

// `build_colour_table`'s HSL→RGB + gamma (the `hsl_to_rgb.glsl` port):
// flat faces must match `Pix3D::colour_table()[shade] / 256.0`.
fn hslToRgb(hsl: u32) -> vec3<f32> {
    let hue = f32((hsl >> 10) & 0x3fu) / 64.0 + 0.0078125;
    let sat = f32((hsl >> 7) & 0x7u) / 8.0 + 0.0625;
    let lum = f32(hsl & 0x7fu);
    let var11 = lum / 128.0;
    var r = var11;
    var g = var11;
    var b = var11;

    let var19 = select(var11 + sat - var11 * sat, var11 * (1.0 + sat), var11 < 0.5);
    let var21 = 2.0 * var11 - var19;
    var var23 = hue + 0.3333333333333333;
    if (var23 > 1.0) { var23 -= 1.0; }
    var var27 = hue - 0.3333333333333333;
    if (var27 < 0.0) { var27 += 1.0; }

    if (6.0 * var23 < 1.0) {
        r = var21 + (var19 - var21) * 6.0 * var23;
    } else if (2.0 * var23 < 1.0) {
        r = var19;
    } else if (3.0 * var23 < 2.0) {
        r = var21 + (var19 - var21) * (0.6666666666666666 - var23) * 6.0;
    } else {
        r = var21;
    }

    if (6.0 * hue < 1.0) {
        g = var21 + (var19 - var21) * 6.0 * hue;
    } else if (2.0 * hue < 1.0) {
        g = var19;
    } else if (3.0 * hue < 2.0) {
        g = var21 + (var19 - var21) * (0.6666666666666666 - hue) * 6.0;
    } else {
        g = var21;
    }

    if (6.0 * var27 < 1.0) {
        b = var21 + (var19 - var21) * 6.0 * var27;
    } else if (2.0 * var27 < 1.0) {
        b = var19;
    } else if (3.0 * var27 < 2.0) {
        b = var21 + (var19 - var21) * (0.6666666666666666 - var27) * 6.0;
    } else {
        b = var21;
    }

    // `build_colour_table` truncates each channel to `(r * 256)` and gamma
    // corrects that quantized value, so mirror the double rounding or dark
    // shades drift well off the CPU table.
    let r0 = floor(r * 256.0) / 256.0;
    let g0 = floor(g * 256.0) / 256.0;
    let b0 = floor(b * 256.0) / 256.0;
    return vec3<f32>(pow(r0, scene.brightness), pow(g0, scene.brightness), pow(b0, scene.brightness));
}

fn shadeVertex(index: u32) -> vec3<i32> {
    let v = vertices[index];
    return vec3<i32>(
        256 + (i32(v.x) << 9u) / i32(v.z),
        167 + (i32(v.y) << 9u) / i32(v.z),
        i32(v.abhsl & 0xffffu),
    );
}

@vertex
fn vs_main(in: VsIn, @builtin(vertex_index) index: u32) -> VsOut {
    var out: VsOut;
    let z = in.pos.z;
    let alpha = f32((in.abhsl >> 24) & 0xffu) / 255.0;
    let hsl = in.abhsl & 0xffffu;
    let tex_id = in.uv_tex & 0xffffu;
    // The packed u/v are signed 16-bit (texture units × 256). Sign-extend
    // so a face whose coordinates cross zero interpolates across the few
    // texels between its vertices; the sampler then clamps U and wraps V
    // per fragment like Java's per-pixel `0..16256` clamp / `& 0x3f80` mask.
    let u = bitcast<i32>(in.uv_tex) >> 16u;
    let v = bitcast<i32>(in.v << 16u) >> 16u;

    // The projection mirrors the CPU `origin + (x << 9) / z` (origin = the
    // 512×334 area_game centre); w = z gives perspective-correct
    // interpolation. There is no depth attachment — the capture's painter
    // order resolves overlap — so clip.z only has to stay inside the clip
    // volume, which it does for every z >= 50 (the CPU near plane).
    out.position = vec4<f32>(in.pos.x * SCALE_X, in.pos.y * SCALE_Y, z - NEAR, z);
    out.color = hslToRgb(hsl);
    out.u = f32(u);
    out.v = f32(v);
    out.tex_id = tex_id;
    out.alpha = alpha;
    // Only the first (provoking) vertex supplies flat triangle metadata.
    // Read the already-uploaded mesh; no expanded vertices or second upload.
    if (tex_id > 0u && index % 3u == 0u) {
        var a = shadeVertex(index);
        var b = shadeVertex(index + 1u);
        var c = shadeVertex(index + 2u);
        if (a.y > b.y) { let t = a; a = b; b = t; }
        if (b.y > c.y) { let t = b; b = c; c = t; }
        if (a.y > b.y) { let t = a; a = b; b = t; }
        out.shade_a = a;
        out.shade_b = b;
        out.shade_c = c;
        out.shade_ab = shadeStep(a, b);
        out.shade_bc = shadeStep(b, c);
        out.shade_ac = shadeStep(a, c);
        out.hclip = (in.abhsl >> 16u) & 1u;
    }
    return out;
}

// Pix3D.texture_triangle advances both X and shade in 16.16 along the
// integer projected edges. Recover the same scanline, not the fragment
// centre's perspective-interpolated shade.
fn shadeStep(a: vec3<i32>, b: vec3<i32>) -> vec2<i32> {
    // Divide per vertex, not per fragment. Flat varyings carry these exact
    // integer gradients from the provoking vertex.
    return ((b.xz - a.xz) << vec2<u32>(16u)) / max(b.y - a.y, 1);
}

fn shadeEdge(a: vec3<i32>, step: vec2<i32>, y: i32) -> vec2<i32> {
    return (a.xz << vec2<u32>(16u)) + step * (y - a.y);
}

fn textureShade(in: VsOut) -> u32 {
    let a = in.shade_a;
    let b = in.shade_b;
    let c = in.shade_c;
    // GPU pixel-centre coverage can extend past the integer CPU edges.
    // Shade those fringe pixels from the nearest CPU scanline/block,
    // never by extrapolating a negative shade into an unsigned shift.
    let y = clamp(i32(in.position.y), a.y, max(a.y, c.y - 1));
    let long = shadeEdge(a, in.shade_ac, y);
    var short = shadeEdge(b, in.shade_bc, y);
    if (y < b.y) { short = shadeEdge(a, in.shade_ab, y); }
    let left = select(short, long, long.x < short.x);
    let right = select(long, short, long.x < short.x);
    var start = left.x >> 16u;
    let end = right.x >> 16u;
    var shade = left.y >> 8u;
    let shade_end = right.y >> 8u;
    var stride = 0;
    if (in.hclip != 0u || min(a.x, min(b.x, c.x)) < 0 || max(a.x, max(b.x, c.x)) > 511) {
        // hclip: derive the per-pixel step before clipping the span.
        let step = (shade_end - shade) / max(end - start, 1);
        if (start < 0) { shade -= start * step; start = 0; }
        stride = step << 12u;
    } else if (end - start > 7) {
        // The unclipped CPU path uses div_table[span / 8], not / span.
        stride = ((shade_end - shade) * (32768 / ((end - start) >> 3u))) >> 6u;
    }
    let block = clamp(i32(in.position.x) - start, 0, max(end - start - 1, 0)) >> 3u;
    let lo = min(a.z, min(b.z, c.z)) << 17u;
    let hi = max(a.z, max(b.z, c.z)) << 17u;
    return u32(clamp((shade << 9u) + block * stride, lo, hi));
}

fn shadeTexel(colour: vec3<f32>, shade: u32) -> vec3<f32> {
    // get_texels bakes packed RGB blocks. Channel-wise float factors lose
    // both the F8F8FF quantization and the packed subtraction/shift carries.
    let channels = vec3<u32>(round(colour * 255.0));
    var rgb = ((channels.r << 16u) | (channels.g << 8u) | channels.b) & 0xf8f8ffu;
    switch ((shade >> 21u) & 3u) {
        case 1u: { rgb -= rgb >> 3u; }
        case 2u: { rgb -= rgb >> 2u; }
        case 3u: { rgb = rgb - (rgb >> 2u) - (rgb >> 3u); }
        default: {}
    }
    rgb = (rgb & 0xf8f8ffu) >> ((shade >> 23u) & 31u);
    return vec3<f32>(f32((rgb >> 16u) & 255u), f32((rgb >> 8u) & 255u), f32(rgb & 255u)) / 255.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.tex_id > 0u) {
        // The `texture_2d_array` has one 128×128 layer per texture id;
        // the packed `tex_id` is texture id + 1. u/v are in 1/256 texture
        // units, so 0..256 maps across the whole layer (u/256 × 128 = u/2), sampled
        // with the layer clamped to the array depth (RuneLite `frag.glsl`
        // samples `textures` by `fTextureId - 1`; ids beyond the array
        // clamp instead of dropping, so textured faces never vanish).
        let id = min(in.tex_id - 1u, 49u);
        // RuneLite `vert.glsl`: `fUv = tex / 256` with no shader clamp.
        // `frag.glsl` samples the mipmapped atlas, then `textureLod(..., 0)`
        // for alpha discard so mips do not punch holes.
        let uv = vec2<f32>(in.u, in.v) / 256.0;
        let t = textureSample(model_atlas, model_sampler, uv, i32(id));
        let t0 = textureSampleLevel(model_atlas, model_sampler, uv, i32(id), 0.0);
        if (t0.a < 1.0) { discard; }
        let shade = textureShade(in);
        // Texture 1's fine ripples and texture 17's sparse animated flecks
        // lose their intended water detail when colour is mip-averaged.
        // Other texture ids retain mip-filtered colour.
        let colour = select(t.rgb, t0.rgb, id == 1u || id == 17u);
        let lit = shadeTexel(colour, shade);
        // Non-opaque CPU spans skip zero after packed shading. A sparse
        // transparent mip can quantize to zero too; do not turn that hole
        // into opaque black. Fully opaque texels may legitimately be black.
        if (t.a < 1.0 && all(lit == vec3<f32>(0.0))) { discard; }
        return vec4<f32>(lit, in.alpha);
    }
    return vec4<f32>(in.color, in.alpha);
}
