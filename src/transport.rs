//! Per-run frame transport (SR-040): the single home for how raw frames travel
//! from frame sources to the encoder — `rgb24` (CPU render backend, the
//! byte-identical historical path) or planar `yuv420p` (GPU render backend,
//! 1.5 bytes/px) — and for every frame-size / pixel-format fact derived from
//! that choice. Leaf module by design: pipeline, image, video, ffmpeg, and
//! cache all import it, so it imports nothing (LLR-075).

/// How raw frames are laid out between frame sources and the FFmpeg encoder
/// for one output run (SR-040). Picked once per output via [`Self::for_backend`]
/// and fixed for the run: a mid-build device-lost degrade converts CPU frames
/// (LLR-079) instead of flipping the transport.
// Implements: LLR-075, SR-040
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameTransport {
    /// 3 bytes/px interleaved RGB — the CPU-backend transport, byte-identical
    /// to the pre-SR-040 path (including its FFmpeg rawvideo args).
    Rgb24,
    /// 1.5 bytes/px planar YUV 4:2:0 in FFmpeg rawvideo order (full Y plane,
    /// then U, then V quarter planes) — the GPU-backend transport.
    Yuv420p,
}

impl FrameTransport {
    /// The transport for a run whose LLR-068 backend selection resolved to
    /// GPU (`true`) or CPU (`false` — selected, or full-run fallback):
    /// Gpu -> `Yuv420p`, Cpu -> `Rgb24` (SR-040 transport-selection leg).
    /// Takes the selection outcome as a bool so this module stays a leaf (the
    /// `RenderBackend` enum lives in `image::backend`); the one production
    /// mapping site is `pipeline`'s `transport_for`.
    // Implements: LLR-075, SR-040
    pub fn for_backend(gpu_backend: bool) -> Self {
        if gpu_backend {
            FrameTransport::Yuv420p
        } else {
            FrameTransport::Rgb24
        }
    }

    /// Exact byte length of one `width`x`height` frame on this transport:
    /// `w*h*3` (rgb24) or `w*h*3/2` (yuv420p — exact because `even_dims`
    /// guarantees even dimensions, LLR-010). Odd dimensions are a caller bug:
    /// debug-asserted, never silently truncated by the `/2`.
    // Implements: LLR-075, SR-040
    pub fn frame_bytes(self, width: u32, height: u32) -> usize {
        debug_assert!(
            width.is_multiple_of(2) && height.is_multiple_of(2),
            "frame_bytes requires even dimensions (even_dims, LLR-010): {width}x{height}"
        );
        let px = (width as usize) * (height as usize);
        match self {
            FrameTransport::Rgb24 => px * 3,
            FrameTransport::Yuv420p => px * 3 / 2,
        }
    }

    /// The FFmpeg pixel-format name for this transport, consumed by the
    /// rawvideo input args (LLR-080) and the decode request (LLR-078):
    /// `rgb24` | `yuv420p`.
    // Implements: LLR-075, SR-040
    pub fn pixel_format(self) -> &'static str {
        match self {
            FrameTransport::Rgb24 => "rgb24",
            FrameTransport::Yuv420p => "yuv420p",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-040, LLR-075 (TC-107) — frame_bytes returns w*h*3 (Rgb24)
    // and w*h*3/2 (Yuv420p) at representative even dims including the 2x2
    // even_dims minimum (TC-046); odd dimensions are guarded (debug-assert),
    // unreachable in production behind even_dims (LLR-010) but never silent;
    // pixel_format returns the strings the LLR-080 args consume.
    #[test]
    fn frame_bytes_and_pixel_format_per_transport_sr040() {
        for (w, h) in [(2u32, 2u32), (320, 200), (1440, 900), (1920, 1080)] {
            assert_eq!(
                FrameTransport::Rgb24.frame_bytes(w, h),
                (w * h * 3) as usize,
                "rgb24 {w}x{h}"
            );
            assert_eq!(
                FrameTransport::Yuv420p.frame_bytes(w, h),
                (w * h * 3 / 2) as usize,
                "yuv420p {w}x{h}"
            );
        }
        // The even_dims minimum explicitly: 2x2 -> 12 / 6 bytes.
        assert_eq!(FrameTransport::Rgb24.frame_bytes(2, 2), 12);
        assert_eq!(FrameTransport::Yuv420p.frame_bytes(2, 2), 6);

        assert_eq!(FrameTransport::Rgb24.pixel_format(), "rgb24");
        assert_eq!(FrameTransport::Yuv420p.pixel_format(), "yuv420p");

        // Odd dims are guarded, never silently truncated (tests run in debug,
        // so the debug_assert fires as a panic).
        for (w, h) in [(321u32, 200u32), (320u32, 201u32)] {
            for t in [FrameTransport::Rgb24, FrameTransport::Yuv420p] {
                assert!(
                    std::panic::catch_unwind(move || t.frame_bytes(w, h)).is_err(),
                    "{t:?}.frame_bytes({w},{h}) must reject odd dimensions"
                );
            }
        }
    }

    // Verifies: SR-040, LLR-075 (TC-107) — gpu -> Yuv420p; cpu (selected or
    // full-run fallback, both of which resolve to backend=Cpu in the LLR-068
    // selection) -> Rgb24. The fn's ONLY input is the pre-run selection
    // outcome — the mid-build device-lost degrade has no input here, so the
    // per-run choice is fixed once (SR-040 one-format-per-run leg; degrade
    // frames convert via LLR-079 instead).
    #[test]
    fn for_backend_picks_once_per_run_sr040() {
        assert_eq!(FrameTransport::for_backend(true), FrameTransport::Yuv420p);
        assert_eq!(FrameTransport::for_backend(false), FrameTransport::Rgb24);
        // Pure and deterministic: same selection input, same transport.
        for gpu in [true, false] {
            assert_eq!(
                FrameTransport::for_backend(gpu),
                FrameTransport::for_backend(gpu)
            );
        }
    }
}
