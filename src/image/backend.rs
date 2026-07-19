//! Render-backend selection (SR-039): a pure decision table over the config
//! `render_backend` request and the process-wide-once wgpu adapter probe —
//! the LLR-046/LLR-048 encoder pattern replayed for the frame renderer. A
//! missing adapter or failed probe always degrades to the CPU renderer with a
//! reason; no combination fails the build.
// Implements: LLR-068, SR-039

use std::sync::OnceLock;

/// Parsed config `render_backend` value (the SR-039 set; validation rejects
/// anything else, so `parse` maps unknown defensively to `Cpu`).
// Implements: LLR-068, SR-039
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendRequest {
    /// GPU when a usable adapter exists, else CPU with a logged reason.
    Auto,
    /// The reference CPU renderer; the wgpu probe is never consulted.
    Cpu,
    /// Explicit GPU; a failed/absent adapter falls back to CPU with a warning.
    Gpu,
}

impl BackendRequest {
    /// Parse the validated config field (`ALLOWED_RENDER_BACKENDS`). Unknown
    /// values — impossible past `Config::validate` — degrade to `Cpu`.
    pub fn parse(s: &str) -> Self {
        match s {
            "auto" => Self::Auto,
            "gpu" => Self::Gpu,
            _ => Self::Cpu,
        }
    }
}

/// Outcome of the process-wide wgpu adapter probe.
// Implements: LLR-068, SR-039
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterProbe {
    /// A usable adapter + device; carries the adapter's name for reporting.
    Present(String),
    /// No adapter satisfied the request (e.g. no GPU on this host).
    Absent,
    /// An adapter was found but device creation failed (reason attached).
    Fail(String),
}

/// Which renderer implementation a build uses.
// Implements: LLR-068, SR-039
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackend {
    Cpu,
    Gpu,
}

/// The backend actually selected for a build, plus the reporting inputs: the
/// adapter name when GPU, and the fallback reason when a GPU-capable request
/// degraded to CPU (`None` for a plain CPU selection).
// Implements: LLR-068, LLR-072, SR-039
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSelection {
    pub backend: RenderBackend,
    /// Adapter name, `Some` exactly when `backend == Gpu`.
    pub adapter: Option<String>,
    /// Why a GPU-capable request degraded to CPU; `None` otherwise.
    pub fallback_reason: Option<String>,
}

impl BackendSelection {
    /// The backend-in-use line body (LLR-072 reporting, mirroring
    /// `Selection::describe` for encoders): `gpu (<adapter>)`,
    /// `cpu (fallback: <reason>)`, or `cpu`.
    pub fn describe(&self) -> String {
        match (&self.backend, &self.adapter, &self.fallback_reason) {
            (RenderBackend::Gpu, Some(name), _) => format!("gpu ({name})"),
            (_, _, Some(reason)) => format!("cpu (fallback: {reason})"),
            _ => "cpu".to_string(),
        }
    }
}

/// Pure decision table realizing the SR-039 selection/fallback acceptance
/// criteria (cited, not restated). `probe` is a lazy closure so a `cpu`
/// request never touches wgpu; there is deliberately no error variant — a
/// missing adapter never fails the build (SR-039).
// Implements: LLR-068, SR-039
pub fn select_backend<F: FnOnce() -> AdapterProbe>(
    request: BackendRequest,
    probe: F,
) -> BackendSelection {
    fn cpu(reason: Option<String>) -> BackendSelection {
        BackendSelection {
            backend: RenderBackend::Cpu,
            adapter: None,
            fallback_reason: reason,
        }
    }

    // cpu: the probe closure is never invoked (LLR-068 — wgpu untouched).
    if request == BackendRequest::Cpu {
        return cpu(None);
    }
    // Probe-fail folds with absent (SR-039 AC): both degrade to CPU; the
    // reason carries the distinction for the log line.
    let probe_summary = match probe() {
        AdapterProbe::Present(name) => {
            return BackendSelection {
                backend: RenderBackend::Gpu,
                adapter: Some(name),
                fallback_reason: None,
            }
        }
        AdapterProbe::Absent => "no usable GPU adapter".to_string(),
        AdapterProbe::Fail(reason) => format!("GPU adapter probe failed: {reason}"),
    };
    match request {
        // Explicit gpu degraded: the reason names the requested backend and
        // the probe outcome — the caller logs it as a warning (SR-039).
        BackendRequest::Gpu => cpu(Some(format!(
            "render_backend=gpu requested but {probe_summary}; using the cpu renderer"
        ))),
        _ => cpu(Some(format!("{probe_summary} (render_backend=auto)"))),
    }
}

/// Process-wide adapter probe, cached so multi-output builds probe once
/// (LLR-068). Runs only when the request could select GPU — `select_for`
/// passes it lazily.
// Implements: LLR-068, SR-039
pub fn probe_adapter() -> &'static AdapterProbe {
    static PROBE: OnceLock<AdapterProbe> = OnceLock::new();
    PROBE.get_or_init(|| match crate::image::gpu::context() {
        Ok(ctx) => AdapterProbe::Present(ctx.adapter_name.clone()),
        Err(e) if e.absent => AdapterProbe::Absent,
        Err(e) => AdapterProbe::Fail(e.reason.clone()),
    })
}

/// Resolve the backend for a validated config `render_backend` field: parse
/// the request and apply [`select_backend`] over the cached process-wide
/// probe. The single selection entry point for the build pipeline.
// Implements: LLR-068, SR-039
pub fn select_for(render_backend: &str) -> BackendSelection {
    select_backend(BackendRequest::parse(render_backend), || {
        probe_adapter().clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-039, LLR-068 (TC-095) — the full SR-039 Permutations
    // product (render_backend x adapter, @full — 6 combos, probe-fail folded
    // with absent per the SR AC): the pure decision table selects
    // deterministically, cpu never invokes the probe, and NO combination
    // returns an error/abort (the return type has no error variant — a
    // missing adapter never fails the build). Mirrors the TC-065 pattern.
    #[test]
    fn select_backend_matrix_sr039() {
        let present = || AdapterProbe::Present("NVIDIA GeForce RTX 3080".into());
        let fail = || AdapterProbe::Fail("device request failed".into());

        // unset/auto + present -> gpu, no fallback reason, adapter named.
        let s = select_backend(BackendRequest::parse("auto"), present);
        assert_eq!(s.backend, RenderBackend::Gpu);
        assert_eq!(s.adapter.as_deref(), Some("NVIDIA GeForce RTX 3080"));
        assert!(s.fallback_reason.is_none());
        assert_eq!(s.describe(), "gpu (NVIDIA GeForce RTX 3080)");

        // auto + absent -> cpu with a logged reason.
        let s = select_backend(BackendRequest::Auto, || AdapterProbe::Absent);
        assert_eq!(s.backend, RenderBackend::Cpu);
        assert!(s.adapter.is_none());
        let reason = s.fallback_reason.expect("auto+absent carries a reason");
        assert!(
            reason.contains("adapter"),
            "reason names the cause: {reason}"
        );
        // auto + probe-fail folds with absent (cpu + reason incl. the failure).
        let s = select_backend(BackendRequest::Auto, fail);
        assert_eq!(s.backend, RenderBackend::Cpu);
        let reason = s.fallback_reason.expect("auto+fail carries a reason");
        assert!(
            reason.contains("device request failed"),
            "reason carries the probe failure: {reason}"
        );

        // cpu + any: cpu, no reason, and the probe is NEVER invoked.
        let s = select_backend(BackendRequest::parse("cpu"), || {
            panic!("cpu must never invoke the wgpu probe")
        });
        assert_eq!(s.backend, RenderBackend::Cpu);
        assert!(s.fallback_reason.is_none(), "cpu is never a fallback");
        assert_eq!(s.describe(), "cpu");

        // gpu + present -> gpu.
        let s = select_backend(BackendRequest::parse("gpu"), present);
        assert_eq!(s.backend, RenderBackend::Gpu);
        assert!(s.fallback_reason.is_none());

        // gpu + absent/probe-fail -> cpu with a warning reason naming the
        // requested backend and the probe outcome.
        for (probe, expect_in_reason) in [
            (AdapterProbe::Absent, "adapter"),
            (
                AdapterProbe::Fail("device request failed".into()),
                "device request failed",
            ),
        ] {
            let s = select_backend(BackendRequest::Gpu, || probe.clone());
            assert_eq!(s.backend, RenderBackend::Cpu);
            let d = s.describe();
            assert!(d.starts_with("cpu (fallback:"), "{d}");
            let reason = s.fallback_reason.expect("gpu fallback carries a reason");
            assert!(reason.contains("gpu"), "reason names the backend: {reason}");
            assert!(
                reason.contains(expect_in_reason),
                "reason names the probe outcome: {reason}"
            );
        }

        // Unknown request text (defensive; validation rejects it upstream)
        // degrades to cpu rather than panicking.
        assert_eq!(
            select_backend(BackendRequest::parse("cuda"), present).backend,
            RenderBackend::Cpu
        );
    }
}
