//! GPU frame renderer (SR-039): process-wide wgpu device/queue, once-per-clip
//! texture upload, per-frame textured-quad draws through CPU-computed
//! [`ClipPlan`] uniforms, and a 3-deep async staging-buffer readback ring
//! emitting `rgb24` frames shape-identical to the CPU path. A device error
//! mid-build degrades to the CPU renderer from the retained [`LoadedClip`] —
//! never a skip, never a failed build (LLR-072).

use crate::image::{ClipRenderer, FrameRenderer, LoadTimings, LoadedClip};
use crate::transform::ClipPlan;
use bytemuck::{Pod, Zeroable};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};

/// Why the process-wide GPU context could not be created.
// Implements: LLR-068, SR-039
#[derive(Debug, Clone)]
pub struct InitError {
    /// True when no adapter satisfied the request (vs a device-creation
    /// failure) — the LLR-068 `Absent` vs `Fail` classification input.
    pub absent: bool,
    pub reason: String,
}

/// Process-wide wgpu device/queue plus the immutable render plumbing shared
/// by every clip and output (pipeline, layout, sampler). Created once,
/// pollster-blocked (LLR-069); `context()` memoizes the result.
// Implements: LLR-069, SR-039
pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Adapter name for the backend-in-use report (LLR-072).
    pub adapter_name: String,
    /// Largest 2D texture dimension the device accepts; larger prescales
    /// render on the CPU instead (defensive — prescale is output-scale).
    max_texture_dim: u32,
}

impl GpuContext {
    /// True when `plan`'s prescaled source fits the device texture limit.
    pub fn fits_texture(&self, plan: &ClipPlan) -> bool {
        plan.pre_w <= self.max_texture_dim && plan.pre_h <= self.max_texture_dim
    }
}

/// The memoized process-wide context: one adapter probe + device per process
/// regardless of output count (LLR-068/LLR-069). `Err` is the classified
/// probe failure consumed by `backend::probe_adapter`.
// Implements: LLR-068, LLR-069, SR-039
pub fn context() -> Result<&'static GpuContext, &'static InitError> {
    static CONTEXT: OnceLock<Result<GpuContext, InitError>> = OnceLock::new();
    CONTEXT.get_or_init(init_context).as_ref()
}

/// True once a device loss has degraded the build: every later clip selects
/// the CPU renderer without re-probing (LLR-072 recorded call).
// Implements: LLR-072, SR-039
pub fn degraded() -> bool {
    DEGRADED.load(Ordering::Relaxed)
}

static DEGRADED: AtomicBool = AtomicBool::new(false);

/// Flag the process as GPU-degraded, logging one warning naming the reason
/// (only the first caller logs — LLR-072's single-warning contract).
fn set_degraded(reason: &str) {
    if !DEGRADED.swap(true, Ordering::Relaxed) {
        log::warn!(
            "GPU device error mid-build ({reason}); completing this clip on the CPU renderer \
             and rendering all remaining clips on cpu"
        );
    }
}

/// Create instance → adapter → device once. DX12/Vulkan only (LLR-074 feature
/// trim), high-performance adapter preference; `WGPU_BACKEND` is honored (the
/// upstream override convention — also the CI-safe no-adapter test seam).
fn init_context() -> Result<GpuContext, InitError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: (wgpu::Backends::DX12 | wgpu::Backends::VULKAN).with_env(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| InitError {
        absent: true,
        reason: format!("no usable GPU adapter: {e}"),
    })?;
    let adapter_name = adapter.get_info().name;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("slideshow render device"),
        ..Default::default()
    }))
    .map_err(|e| InitError {
        absent: false,
        reason: format!("device request failed on '{adapter_name}': {e}"),
    })?;

    // Never panic on async device errors (wgpu's default handler does): a
    // mid-build device loss must degrade, not crash (LLR-072). The poll/map
    // errors in the readback path are the degrade trigger.
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
        log::warn!("wgpu device error: {e}");
    }));
    device.set_device_lost_callback(|reason, message| {
        set_degraded(&format!("device lost ({reason:?}): {message}"));
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("render.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("render.wgsl").into()),
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("clip bind layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<FrameUniforms>() as u64
                    ),
                },
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("clip pipeline layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("clip render pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    // Bilinear + clamp-to-edge: the hardware analog of the CPU warp's
    // bilinear sampling (LLR-069).
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("clip sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let max_texture_dim = device.limits().max_texture_dimension_2d;

    Ok(GpuContext {
        device,
        queue,
        pipeline,
        bind_layout,
        sampler,
        adapter_name,
        max_texture_dim,
    })
}

/// Per-frame uniforms: row-major affine rows mapping an output fragment
/// position (pixel-center coordinates) to a normalized source-texture UV.
/// Computed on the **CPU** from [`ClipPlan::projection`] — the shader derives
/// no motion math (SR-022 backend-independent seeded motion). `.w` lanes pad
/// to vec4 alignment.
// Implements: LLR-070, SR-039, SR-022
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FrameUniforms {
    pub row_x: [f32; 4],
    pub row_y: [f32; 4],
}

/// Build frame `i`'s uniforms: invert the same render→source projection the
/// CPU warp applies, compose the constant rotation-margin center-crop offset
/// (identical integer math to the CPU crop), then fold in the half-texel
/// offsets converting pixel-center fragment coordinates to texel-center UVs.
/// Pure — unit-tested against the `ClipPlan` ground truth without a device.
// Implements: LLR-070, SR-039, SR-022
pub fn frame_uniforms(plan: &ClipPlan, i: u32) -> FrameUniforms {
    let inv = plan.projection(i).invert();
    // Same integer center-crop offset as the CPU path's crop_imm.
    let off_x = ((plan.render_w - plan.out_w) / 2) as f32;
    let off_y = ((plan.render_h - plan.out_h) / 2) as f32;

    // The composed map is affine (scale/translate/rotate only), so three
    // point images determine it exactly.
    let o = inv * (off_x, off_y);
    let px = inv * (off_x + 1.0, off_y);
    let py = inv * (off_x, off_y + 1.0);
    let (a, b, c) = (px.0 - o.0, py.0 - o.0, o.0);
    let (d, e, f) = (px.1 - o.1, py.1 - o.1, o.1);

    // Fragment position is (x+0.5, y+0.5) for output pixel (x,y); the CPU
    // samples the source at s = inv*(x+off, y+off) with texel centers at
    // integer coords, so uv = (s + 0.5) / tex_dim samples identically.
    let pw = plan.pre_w as f32;
    let ph = plan.pre_h as f32;
    let cx = c - 0.5 * a - 0.5 * b + 0.5;
    let cy = f - 0.5 * d - 0.5 * e + 0.5;
    FrameUniforms {
        row_x: [a / pw, b / pw, cx / pw, 0.0],
        row_y: [d / ph, e / ph, cy / ph, 0.0],
    }
}

/// Expand an `rgb24` buffer to `rgba` (alpha 255) for the Rgba8 texture
/// upload — wgpu has no 3-byte texel format (LLR-069). Pure.
// Implements: LLR-069, SR-039
pub fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    debug_assert_eq!(rgb.len() % 3, 0);
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(px);
        out.push(255);
    }
    out
}

/// Strip a mapped readback buffer — rows padded to `padded_bpr` bytes of
/// `rgba` — into a tightly-packed `rgb24` frame of `w`x`h`, byte-shape
/// identical to the CPU path (LLR-071). Pure.
// Implements: LLR-071, SR-039
pub fn strip_readback(padded: &[u8], padded_bpr: usize, w: usize, h: usize) -> Vec<u8> {
    debug_assert!(padded.len() >= padded_bpr * h);
    let mut out = Vec::with_capacity(w * h * 3);
    for row in 0..h {
        let start = row * padded_bpr;
        for px in padded[start..start + w * 4].chunks_exact(4) {
            out.extend_from_slice(&px[..3]);
        }
    }
    out
}

/// Staging-buffer row stride: `w*4` rounded up to wgpu's copy alignment
/// (256-byte rows). Pure.
// Implements: LLR-071, SR-039
pub fn padded_bytes_per_row(width: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (width * 4).div_ceil(align) * align
}

/// Ring bookkeeping: the slot index of the oldest in-flight readback given
/// the next-write index, in-flight count, and ring depth. Pure.
// Implements: LLR-071, SR-039
pub fn oldest_slot(next: usize, pending: usize, depth: usize) -> usize {
    debug_assert!(pending <= depth && pending > 0);
    (next + depth - pending) % depth
}

/// One in-flight readback: its mappable staging buffer and, while pending,
/// the map-completion channel to join on.
struct Slot {
    buffer: wgpu::Buffer,
    rx: Option<mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    submission: Option<wgpu::SubmissionIndex>,
}

/// 3-deep ring of mappable staging buffers: frame `i`'s copy + async map
/// overlap frames `i+1`/`i+2`'s draws; depth 3 caps readback staging memory
/// at `3 * padded_bpr * out_h` bytes (LLR-071).
// Implements: LLR-071, SR-039
struct ReadbackRing {
    slots: Vec<Slot>,
    /// Next slot to submit into.
    next: usize,
    /// Submitted-but-not-drained count.
    pending: usize,
}

/// Readback ring depth (LLR-071).
const RING_DEPTH: usize = 3;

impl ReadbackRing {
    fn new(ctx: &GpuContext, bytes_per_slot: u64) -> Self {
        let slots = (0..RING_DEPTH)
            .map(|i| Slot {
                buffer: ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("readback slot {i}")),
                    size: bytes_per_slot,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                rx: None,
                submission: None,
            })
            .collect();
        Self {
            slots,
            next: 0,
            pending: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.pending == RING_DEPTH
    }

    fn is_empty(&self) -> bool {
        self.pending == 0
    }

    /// The slot the next frame submits into. Caller must drain first if full.
    fn write_slot(&mut self) -> &wgpu::Buffer {
        debug_assert!(!self.is_full());
        &self.slots[self.next].buffer
    }

    /// Mark the current write slot submitted (map requested).
    fn submitted(
        &mut self,
        rx: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
        submission: wgpu::SubmissionIndex,
    ) {
        let slot = &mut self.slots[self.next];
        slot.rx = Some(rx);
        slot.submission = Some(submission);
        self.next = (self.next + 1) % RING_DEPTH;
        self.pending += 1;
    }

    /// Block on the oldest in-flight readback and return its stripped rgb24
    /// frame. `Err(reason)` signals a device error — the caller degrades to
    /// CPU (LLR-072).
    fn drain_oldest(&mut self, ctx: &GpuContext, w: usize, h: usize) -> Result<Vec<u8>, String> {
        let idx = oldest_slot(self.next, self.pending, RING_DEPTH);
        let slot = &mut self.slots[idx];
        let rx = slot.rx.take().expect("pending slot has a receiver");
        let submission = slot.submission.take();
        // Wait for exactly this slot's submission so newer frames keep
        // overlapping (LLR-071).
        ctx.device
            .poll(wgpu::PollType::Wait {
                submission_index: submission,
                timeout: None,
            })
            .map_err(|e| format!("device poll failed: {e}"))?;
        rx.recv()
            .map_err(|_| "map callback dropped (device lost)".to_string())?
            .map_err(|e| format!("staging-buffer map failed: {e}"))?;
        let frame = {
            let view = slot
                .buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|e| format!("mapped-range read failed: {e}"))?;
            strip_readback(&view, padded_bytes_per_row(w as u32) as usize, w, h)
        };
        slot.buffer.unmap();
        self.pending -= 1;
        Ok(frame)
    }
}

/// GPU implementation of [`ClipRenderer`]: uploads the clip's prescaled image
/// once as an Rgba8 texture, then per frame draws one textured quad through
/// the CPU-computed uniforms into an `out_w`x`out_h` target (margin crop
/// folded into the matrix — no oversized canvas) and reads back through the
/// ring. Retains its [`LoadedClip`] so a device loss completes the in-flight
/// range on a CPU [`FrameRenderer`] over the SAME prescale/plan (LLR-072).
// Implements: LLR-069, LLR-070, LLR-071, LLR-072, SR-039
pub struct GpuRenderer {
    ctx: &'static GpuContext,
    clip: LoadedClip,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    ring: ReadbackRing,
    /// Set on the first device error: all remaining ranges render here.
    cpu_fallback: Option<FrameRenderer>,
}

impl GpuRenderer {
    /// Upload `clip`'s prescaled image (rgb→rgba expand) once and prepare the
    /// per-frame plumbing. The ~10–15 MB upload amortizes over the clip's
    /// frames (LLR-069).
    pub fn new(ctx: &'static GpuContext, clip: LoadedClip) -> Self {
        let (pre_w, pre_h) = (clip.plan.pre_w, clip.plan.pre_h);
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("clip texture"),
            size: wgpu::Extent3d {
                width: pre_w,
                height: pre_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgb_to_rgba(clip.prescaled.as_raw()),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pre_w * 4),
                rows_per_image: Some(pre_h),
            },
            wgpu::Extent3d {
                width: pre_w,
                height: pre_h,
                depth_or_array_layers: 1,
            },
        );

        let uniforms = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tex_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clip bind group"),
            layout: &ctx.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&ctx.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });

        // Draw target: the output size directly — the rotation-margin crop is
        // folded into the uniforms, so fewer readback bytes (LLR-070).
        let (out_w, out_h) = (clip.plan.out_w, clip.plan.out_h);
        let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame target"),
            size: wgpu::Extent3d {
                width: out_w,
                height: out_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let ring = ReadbackRing::new(ctx, padded_bytes_per_row(out_w) as u64 * out_h as u64);

        Self {
            ctx,
            clip,
            bind_group,
            uniforms,
            target,
            target_view,
            ring,
            cpu_fallback: None,
        }
    }

    /// Submit frame `i`: write its uniforms, draw the quad, queue the copy to
    /// the ring's write slot, and request the async map.
    fn submit_frame(&mut self, i: u32) {
        let u = frame_uniforms(&self.clip.plan, i);
        self.ctx
            .queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.ctx.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let (out_w, out_h) = (self.clip.plan.out_w, self.clip.plan.out_h);
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: self.ring.write_slot(),
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row(out_w)),
                    rows_per_image: Some(out_h),
                },
            },
            wgpu::Extent3d {
                width: out_w,
                height: out_h,
                depth_or_array_layers: 1,
            },
        );
        let submission = self.ctx.queue.submit([enc.finish()]);
        let (tx, rx) = mpsc::channel();
        self.ring
            .write_slot()
            .map_async(wgpu::MapMode::Read, .., move |r| {
                let _ = tx.send(r);
            });
        self.ring.submitted(rx, submission);
    }

    /// The GPU render loop for one range; `Err(reason)` = device error, the
    /// caller (the trait impl) degrades to CPU.
    fn try_render_range(&mut self, start: u32, end: u32) -> Result<Vec<Vec<u8>>, String> {
        let (w, h) = (self.clip.plan.out_w as usize, self.clip.plan.out_h as usize);
        let mut frames = Vec::with_capacity((end - start) as usize);
        for i in start..end {
            if self.ring.is_full() {
                frames.push(self.ring.drain_oldest(self.ctx, w, h)?);
            }
            self.submit_frame(i);
        }
        while !self.ring.is_empty() {
            frames.push(self.ring.drain_oldest(self.ctx, w, h)?);
        }
        Ok(frames)
    }
}

impl ClipRenderer for GpuRenderer {
    fn total_frames(&self) -> u32 {
        self.clip.plan.total_frames
    }

    // Implements: LLR-072, SR-039 — a device error completes the in-flight
    // range on a CPU FrameRenderer built from the SAME LoadedClip (identical
    // crop/projection, seam-free within the LLR-073 tolerance) and flags the
    // process degraded so later clips select CPU without re-probing.
    fn render_range(&mut self, start: u32, end: u32) -> Vec<Vec<u8>> {
        if let Some(cpu) = &self.cpu_fallback {
            return cpu.render_range(start, end);
        }
        match self.try_render_range(start, end) {
            Ok(frames) => frames,
            Err(reason) => {
                set_degraded(&reason);
                let cpu = FrameRenderer::new(self.clip.clone());
                let frames = cpu.render_range(start, end);
                self.cpu_fallback = Some(cpu);
                frames
            }
        }
    }

    fn load_timings(&self) -> LoadTimings {
        self.clip.timings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputDef;
    use crate::transform::seed_from_str;

    fn out_def(rotation: f32) -> OutputDef {
        OutputDef {
            name: "t".into(),
            width: 320,
            height: 200,
            fps: 30,
            pic_display_time_secs: 1.0,
            fade_time_secs: 0.1,
            max_rotation_degrees: rotation,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            encoder: "software".into(),
            x264_preset: "medium".into(),
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    // Verifies: SR-039, SR-022, LLR-070 — the CPU-computed uniforms map an
    // output pixel center to exactly the source texel (as a texel-center UV)
    // that the CPU warp samples for that pixel: inverted projection plus the
    // integer center-crop offset. CI-safe: pure math, no device.
    #[test]
    fn frame_uniforms_match_clip_plan_projection_sr039() {
        let out = out_def(12.0); // rotation on -> margin crop is non-trivial
        let plan = crate::transform::ClipPlan::with_focus(
            1920,
            1280,
            &out,
            seed_from_str("uniforms-test"),
            None,
        );
        assert!(
            plan.render_w > plan.out_w,
            "rotation must produce a margin for this test to bite"
        );
        let off_x = ((plan.render_w - plan.out_w) / 2) as f32;
        let off_y = ((plan.render_h - plan.out_h) / 2) as f32;

        for i in [0, plan.total_frames / 2, plan.total_frames - 1] {
            let u = frame_uniforms(&plan, i);
            let inv = plan.projection(i).invert();
            for (x, y) in [
                (0u32, 0u32),
                (plan.out_w - 1, 0),
                (0, plan.out_h - 1),
                (plan.out_w - 1, plan.out_h - 1),
                (plan.out_w / 2, plan.out_h / 2),
            ] {
                // Ground truth: the CPU warp samples source texel s for
                // output pixel (x,y) after the center crop.
                let s = inv * (x as f32 + off_x, y as f32 + off_y);
                let expect_uv = (
                    (s.0 + 0.5) / plan.pre_w as f32,
                    (s.1 + 0.5) / plan.pre_h as f32,
                );
                // The shader evaluates the rows at the fragment center.
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let got_uv = (
                    u.row_x[0] * fx + u.row_x[1] * fy + u.row_x[2],
                    u.row_y[0] * fx + u.row_y[1] * fy + u.row_y[2],
                );
                let d0 = (got_uv.0 - expect_uv.0).abs();
                let d1 = (got_uv.1 - expect_uv.1).abs();
                assert!(
                    d0 < 1e-5 && d1 < 1e-5,
                    "frame {i} pixel ({x},{y}): uniforms uv {got_uv:?} vs plan uv {expect_uv:?}"
                );
            }
        }
    }

    // Verifies: SR-039, LLR-069, LLR-071 — the pure conversion seams the GPU
    // path shares with nothing else: rgb->rgba expand, padded-readback strip
    // (256-byte row alignment), and ring index bookkeeping. CI-safe.
    #[test]
    fn readback_conversions_roundtrip_sr039() {
        // rgb -> rgba: alpha 255 interleaved.
        assert_eq!(
            rgb_to_rgba(&[1, 2, 3, 4, 5, 6]),
            vec![1, 2, 3, 255, 4, 5, 6, 255]
        );

        // Padded stride: 256-byte alignment boundaries.
        assert_eq!(padded_bytes_per_row(64), 256); // 256 exactly
        assert_eq!(padded_bytes_per_row(65), 512); // just over -> next step
        assert_eq!(padded_bytes_per_row(320), 1280); // already aligned

        // strip_readback: padding bytes and alpha both dropped, row order kept.
        let (w, h) = (2usize, 2usize);
        let bpr = padded_bytes_per_row(w as u32) as usize;
        let mut padded = vec![0xEEu8; bpr * h];
        #[allow(clippy::needless_range_loop)]
        for row in 0..h {
            for col in 0..w {
                let p = row * bpr + col * 4;
                let v = (row * 10 + col) as u8;
                padded[p..p + 4].copy_from_slice(&[v, v + 1, v + 2, 255]);
            }
        }
        assert_eq!(
            strip_readback(&padded, bpr, w, h),
            vec![0, 1, 2, 1, 2, 3, 10, 11, 12, 11, 12, 13]
        );

        // Ring bookkeeping: oldest slot rotates behind the write index.
        assert_eq!(oldest_slot(0, 3, 3), 0); // full ring, next wrapped
        assert_eq!(oldest_slot(1, 1, 3), 0);
        assert_eq!(oldest_slot(2, 2, 3), 0);
        assert_eq!(oldest_slot(0, 1, 3), 2);
    }

    // Verifies: SR-039, SR-022, LLR-066, LLR-070, LLR-073 (TC-103, Release
    // leg) — the GPU renderer's frames stay within the TC-103 epsilon of the
    // CPU reference on every frame of a rotated clip: both backends sample
    // the same prescaled pixels through the same ClipPlan matrix, so residual
    // difference is GPU bilinear weight quantization + the rgba round-trip.
    #[test]
    #[ignore = "Release tier (TC-103): needs a GPU adapter"]
    fn cpu_vs_gpu_within_tolerance_sr039() {
        let dir = std::env::temp_dir().join("slideshow_test_gpu_tol");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grad.png");
        let img = ::image::RgbImage::from_fn(1600, 1200, |x, y| {
            ::image::Rgb([
                (x % 256) as u8,
                (y % 256) as u8,
                ((x * 7 + y * 13) % 256) as u8,
            ])
        });
        img.save(&path).unwrap();

        let out = out_def(15.0); // the canonical rotation-15 shape
        let clip = LoadedClip::load(&path, &out, None).unwrap();
        let ctx = context().expect("this test requires a wgpu adapter");

        let cpu = FrameRenderer::new(clip.clone());
        let mut gpu = GpuRenderer::new(ctx, clip);
        let total = cpu.total_frames();
        assert_eq!(ClipRenderer::total_frames(&gpu), total);

        let cpu_frames = cpu.render_range(0, total);
        let gpu_frames = ClipRenderer::render_range(&mut gpu, 0, total);
        assert!(
            gpu.cpu_fallback.is_none(),
            "GPU path must not have silently degraded to CPU mid-test"
        );
        assert_eq!(gpu_frames.len(), cpu_frames.len());
        // TC-103 epsilon: mean absolute per-channel difference <= 2.0 of 255
        // per frame (the epsilon lives in the TC row; asserted here).
        let mut worst = 0.0f64;
        for (i, (c, g)) in cpu_frames.iter().zip(&gpu_frames).enumerate() {
            let d = crate::image::frame_mean_abs_diff(c, g);
            worst = worst.max(d);
            assert!(
                d <= 2.0,
                "frame {i}: mean abs diff {d} > TC-103 epsilon 2.0"
            );
        }
        eprintln!("cpu_vs_gpu_within_tolerance_sr039: worst per-frame mean abs diff = {worst:.4}");
    }
}
