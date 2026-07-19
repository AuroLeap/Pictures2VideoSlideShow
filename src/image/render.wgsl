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
