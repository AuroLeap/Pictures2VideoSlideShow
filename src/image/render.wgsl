// GPU frame render shader (SR-039, LLR-070): draw one textured quad sampling
// the clip texture through a CPU-computed affine matrix. The matrix comes from
// ClipPlan::projection(i) inverted on the CPU (with the constant rotation-
// margin center-crop offset and the texel->UV normalization folded in) — this
// shader derives NO motion math, so ClipPlan stays the single source of truth
// and seeded motion is backend-independent (SR-022 amendment).

struct FrameUniforms {
    // Row-major affine rows mapping a fragment position (pixel-center
    // coordinates in the output frame) to a normalized source-texture UV:
    //   uv.x = row_x.x * pos.x + row_x.y * pos.y + row_x.z
    //   uv.y = row_y.x * pos.x + row_y.y * pos.y + row_y.z
    // The .w lanes are padding.
    row_x: vec4<f32>,
    row_y: vec4<f32>,
};

@group(0) @binding(0) var clip_tex: texture_2d<f32>;
@group(0) @binding(1) var clip_samp: sampler;
@group(0) @binding(2) var<uniform> u: FrameUniforms;

// Single full-screen triangle covering the output; clipped to the viewport.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let xy = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(
        u.row_x.x * pos.x + u.row_x.y * pos.y + u.row_x.z,
        u.row_y.x * pos.x + u.row_y.y * pos.y + u.row_y.z,
    );
    // Hardware bilinear sampling (clamp-to-edge) replaces the CPU warp's
    // per-pixel resample — the LLR-069 rationale.
    return textureSample(clip_tex, clip_samp, uv);
}

// --- yuv420p transport compute pass (SR-040, LLR-076) -----------------------
// Second pass after the textured-quad draw: reads the rendered frame and emits
// the tightly-packed planar yuv420p rawvideo byte stream (Y w*h, then U, then
// V quarter planes) into a storage buffer. Every colorimetry constant arrives
// as a uniform from src/image/yuv.rs (LLR-079) — this shader carries NO copied
// coefficients or offsets (TC-109). Each invocation packs one u32 word (4
// consecutive output bytes, plane/row boundaries included), so no two
// invocations touch the same word and no atomics are needed.

struct YuvUniforms {
    // [r, g, b, offset] per output plane, BT.601 limited range, from the
    // LLR-079 single home. Inputs to the rows are 0..=255 channel values.
    y_row: vec4<f32>,
    u_row: vec4<f32>,
    v_row: vec4<f32>,
    // x = output width, y = output height (pixels); z/w pad.
    size: vec4<u32>,
};

@group(0) @binding(3) var yuv_src: texture_2d<f32>;
@group(0) @binding(4) var<uniform> yc: YuvUniforms;
@group(0) @binding(5) var<storage, read_write> yuv_out: array<u32>;

// One output byte of the planar stream: Y from its own texel; U/V from the
// 2x2 rgb average (matching the CPU converter's subsampling) through the
// uniform row. floor(v + 0.5) mirrors the converter's round-to-nearest.
fn yuv_byte(b: u32) -> u32 {
    let w = yc.size.x;
    let ysize = w * yc.size.y;
    var v: f32;
    if (b < ysize) {
        let rgb = textureLoad(yuv_src, vec2<u32>(b % w, b / w), 0).rgb * 255.0;
        v = dot(rgb, yc.y_row.xyz) + yc.y_row.w;
    } else {
        let qsize = ysize / 4u;
        let i = (b - ysize) % qsize;
        let half_w = w / 2u;
        let c = vec2<u32>((i % half_w) * 2u, (i / half_w) * 2u);
        let rgb = (textureLoad(yuv_src, c, 0).rgb
            + textureLoad(yuv_src, c + vec2<u32>(1u, 0u), 0).rgb
            + textureLoad(yuv_src, c + vec2<u32>(0u, 1u), 0).rgb
            + textureLoad(yuv_src, c + vec2<u32>(1u, 1u), 0).rgb) * (255.0 / 4.0);
        let row = select(yc.u_row, yc.v_row, (b - ysize) >= qsize);
        v = dot(rgb, row.xyz) + row.w;
    }
    return u32(clamp(floor(v + 0.5), 0.0, 255.0));
}

@compute @workgroup_size(256)
fn cs_yuv420p(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = (yc.size.x * yc.size.y * 3u) / 2u;
    let base = gid.x * 4u;
    if (base >= total) {
        return;
    }
    var packed = 0u;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let b = base + k;
        if (b < total) {
            packed = packed | (yuv_byte(b) << (8u * k));
        }
    }
    yuv_out[gid.x] = packed;
}
