//! Warm-build segment planner (SR-037): pure frame-layout arithmetic mirroring
//! `pipeline::CrossfadeMixer`'s streaming behavior, and the hit/miss partition
//! of an ordered album against the segment store's key set. The mixer is the
//! behavioral reference; `clip_layout` is the closed form of its math, and the
//! `cached_audio_start_frames_match_streaming_capture_sr037` test pins the two
//! together so they cannot drift.

use super::{combine_keys, segment_key, EncodeParams, SegmentKeyInput, SourceIdentity};
use crate::ffmpeg::audio::AudioClip;
use std::ops::Range;
use std::path::PathBuf;

/// The planner's per-clip facts: identity + focus (the key inputs), whether
/// the clip is an image (frame count computable) and its best-known source
/// frame count, and whether it carries audio (LLR-057 timeline).
// Implements: LLR-055, SR-037
#[derive(Debug, Clone)]
pub struct ClipFacts {
    pub identity: SourceIdentity,
    /// Ken Burns focus actually applied (images; `None` = default pan).
    pub focus: Option<(f32, f32)>,
    /// Source frames this clip yields at the output fps. Exact for images
    /// (`OutputDef::total_frames`) and for previously-encoded videos (store
    /// `clip_frames` record); an estimate (`round(duration*fps)`) for a video
    /// not seen before — self-corrected after its first encode.
    pub frames: u64,
    /// Exact (image / recorded) vs estimated (first-seen video) `frames`.
    pub frames_exact: bool,
    pub has_audio: bool,
}

/// Closed-form frame layout of the streaming mixer over per-clip source frame
/// counts `c` with `fade` overlap frames `n` (see `CrossfadeMixer::add_clip`):
///
/// - tail held after clip `j`: `p_j = clamp(c_j - n, 0, n)`
/// - frames emitted during clip `j`'s `add_clip`: `c_j - p_j`
/// - clip `j`'s output start frame (`mixer.emitted()` before `add_clip`):
///   `E_j = Σ_{i<j} (c_i - p_i)`
/// - transition overlap into clip `j`: `m_j = min(p_{j-1}, min(n, c_j))`;
///   a segment boundary (mixer `roll`) exists iff `m_j >= 1`, at output frame
///   `B_j = E_j + floor(m_j / 2)` — the transition midpoint (plan §4)
/// - total emitted: `E_N + p_{N-1}` (the final tail fades out in `finish`).
///
/// A clip with `c <= n` holds no tail, so no boundary precedes its successor —
/// sub-transition-length clips merge into their successor's segment.
// Implements: LLR-055, LLR-057, SR-037
#[derive(Debug, Clone, PartialEq)]
pub struct ClipLayout {
    /// Per-clip output start frame `E_j` (the LLR-040 audio-delay frame).
    pub starts: Vec<u64>,
    /// Per-clip held tail `p_j`.
    pub tails: Vec<u64>,
    /// `(clip index j, output frame B_j)` for every boundary (`m_j >= 1`).
    pub boundaries: Vec<(usize, u64)>,
    /// Total output frames emitted (fade-out included).
    pub total: u64,
}

/// Compute the [`ClipLayout`] for source frame counts `counts` and a `fade`
/// of `n` frames. Pure; O(clips).
// Implements: LLR-055, LLR-057, SR-037
pub fn clip_layout(counts: &[u64], fade: usize) -> ClipLayout {
    let n = fade as u64;
    let mut starts = Vec::with_capacity(counts.len());
    let mut tails: Vec<u64> = Vec::with_capacity(counts.len());
    let mut boundaries = Vec::new();
    let mut emitted: u64 = 0;
    for (j, &c) in counts.iter().enumerate() {
        starts.push(emitted);
        let h = c.min(n);
        let p = c.saturating_sub(n).min(n);
        if j > 0 {
            let m = tails[j - 1].min(h);
            if m >= 1 {
                boundaries.push((j, emitted + m / 2));
            }
        }
        emitted += c - p;
        tails.push(p);
    }
    let total = emitted + tails.last().copied().unwrap_or(0);
    ClipLayout {
        starts,
        tails,
        boundaries,
        total,
    }
}

/// One planned segment: the clip indices it spans (one clip normally; several
/// when sub-transition clips merge), its cache key, whether the store already
/// holds it, and its planned position/extent on the output timeline.
// Implements: LLR-055, SR-037
#[derive(Debug, Clone)]
pub struct PlannedSegment {
    /// Member clip indices (contiguous, in album order).
    pub members: Range<usize>,
    pub key: String,
    pub hit: bool,
    /// For a hit: the stored segment file and its recorded frame count.
    pub stored: Option<(PathBuf, u64)>,
    /// Output frame where this segment begins (segment 0 starts at 0; later
    /// segments start at their opening transition midpoint).
    // lib-API: asserted by the TC-076 planner tests; assembly reads `stored`.
    #[allow(dead_code)]
    pub start_frame: u64,
    /// Planned frame count (exact when every involved clip count is exact).
    // lib-API: asserted by the TC-076 planner tests; assembly reads `stored`.
    #[allow(dead_code)]
    pub frames: u64,
}

/// The ordered segment plan for one output: descriptors plus the underlying
/// clip layout (per-clip start frames — the LLR-057 audio timeline).
// Implements: LLR-055, LLR-057, SR-037
#[derive(Debug, Clone)]
pub struct SegmentPlan {
    pub segments: Vec<PlannedSegment>,
    pub layout: ClipLayout,
}

impl SegmentPlan {
    /// Number of segments already in the store.
    pub fn hits(&self) -> usize {
        self.segments.iter().filter(|s| s.hit).count()
    }

    /// Number of segments that must be (re-)encoded.
    pub fn misses(&self) -> usize {
        self.segments.len() - self.hits()
    }

    /// The audio-bearing clips with their output start frames from the plan's
    /// frame offsets — the cached path's replacement for the streaming
    /// `mixer.emitted()` capture (LLR-040 stays the streaming home).
    // Implements: LLR-057, SR-037, SR-032
    pub fn audio_clips(&self, clips: &[ClipFacts]) -> Vec<AudioClip> {
        audio_clips_from(clips, &self.layout)
    }
}

/// Build the [`AudioClip`] list (source + output start frame) from a layout —
/// shared by the plan and by the post-encode recompute with actual counts.
// Implements: LLR-057, SR-032, SR-037
pub fn audio_clips_from(clips: &[ClipFacts], layout: &ClipLayout) -> Vec<AudioClip> {
    clips
        .iter()
        .zip(layout.starts.iter())
        .filter(|(c, _)| c.has_audio)
        .map(|(c, &start_frame)| AudioClip {
            source: PathBuf::from(&c.identity.path),
            start_frame,
        })
        .collect()
}

/// Group clips into segments from a layout's boundaries: a new segment starts
/// at clip 0 and at every boundary clip. Pure.
// Implements: LLR-055, SR-037
pub fn segment_groups(clip_count: usize, layout: &ClipLayout) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    for &(j, _) in &layout.boundaries {
        groups.push(start..j);
        start = j;
    }
    if clip_count > 0 {
        groups.push(start..clip_count);
    }
    groups
}

/// The combined cache key for the segment spanning `members`, from per-clip
/// [`segment_key`]s (each keyed with its album-adjacent neighbors — SR-037
/// transition partners — regardless of grouping).
// Implements: LLR-053, LLR-055, SR-037
pub fn group_key(
    clips: &[ClipFacts],
    members: &Range<usize>,
    params: &EncodeParams,
    engine: &str,
) -> String {
    let member_keys: Vec<String> = members
        .clone()
        .map(|i| {
            segment_key(&SegmentKeyInput {
                source: &clips[i].identity,
                focus: clips[i].focus,
                left: i.checked_sub(1).map(|l| &clips[l].identity),
                right: clips.get(i + 1).map(|c| &c.identity),
                params,
                engine,
            })
        })
        .collect();
    combine_keys(&member_keys)
}

/// Map an ordered album to its segment plan: compute the layout from the
/// best-known frame counts, group clips at transition midpoints, derive each
/// group's key, and partition hit/miss via `lookup` (the store's key set —
/// returns the stored file + frame count for a present, valid entry). Pure
/// given `lookup`, so hit/miss decisions are unit-testable without ffmpeg.
// Implements: LLR-055, SR-037
pub fn plan_segments<F>(
    clips: &[ClipFacts],
    fade: usize,
    params: &EncodeParams,
    engine: &str,
    mut lookup: F,
) -> SegmentPlan
where
    F: FnMut(&str) -> Option<(PathBuf, u64)>,
{
    let counts: Vec<u64> = clips.iter().map(|c| c.frames).collect();
    let layout = clip_layout(&counts, fade);
    let groups = segment_groups(clips.len(), &layout);

    // Segment start frames: 0 for the first, then the boundary positions.
    let mut starts: Vec<u64> = vec![0];
    starts.extend(layout.boundaries.iter().map(|&(_, b)| b));

    let segments = groups
        .into_iter()
        .zip(starts.iter())
        .enumerate()
        .map(|(gi, (members, &start_frame))| {
            let key = group_key(clips, &members, params, engine);
            let stored = lookup(&key);
            let end_frame = layout
                .boundaries
                .get(gi)
                .map(|&(_, b)| b)
                .unwrap_or(layout.total);
            PlannedSegment {
                hit: stored.is_some(),
                stored,
                members,
                key,
                start_frame,
                frames: end_frame - start_frame,
            }
        })
        .collect();

    SegmentPlan { segments, layout }
}

/// Best-known source frame count for a video that has no recorded actual yet:
/// its probed duration resampled at the output fps (never 0 so the layout
/// stays sane; the first encode records the actual).
// Implements: LLR-055, SR-037
pub fn estimate_video_frames(duration_secs: Option<f32>, fps: u32) -> u64 {
    ((duration_secs.unwrap_or(0.0) as f64 * fps as f64).round() as u64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputDef;
    use crate::pipeline::mixer_test_probe;
    use std::collections::HashMap;

    fn params() -> EncodeParams {
        EncodeParams {
            width: 320,
            height: 240,
            fps: 24,
            quality_crf: 28,
            pic_display_time_secs: 1.0,
            fade_time_secs: 0.25,
            max_rotation_degrees: 0.0,
            zoom_amount: 0.12,
            ken_burns: true,
            encoder: "libx264".into(),
            x264_preset: "veryfast".into(),
            transport: crate::transport::FrameTransport::Rgb24,
        }
    }

    fn clip(path: &str, frames: u64, has_audio: bool) -> ClipFacts {
        ClipFacts {
            identity: SourceIdentity {
                path: path.into(),
                size: 100,
                mtime_ms: 1,
            },
            focus: None,
            frames,
            frames_exact: true,
            has_audio,
        }
    }

    fn album(names_frames: &[(&str, u64)]) -> Vec<ClipFacts> {
        names_frames
            .iter()
            .map(|(n, f)| clip(n, *f, false))
            .collect()
    }

    /// Plan against a store snapshot (key -> frames map).
    fn plan(clips: &[ClipFacts], fade: usize, store: &HashMap<String, u64>) -> SegmentPlan {
        plan_segments(clips, fade, &params(), "t+f1", |k| {
            store
                .get(k)
                .map(|&frames| (PathBuf::from(format!("{k}.ts")), frames))
        })
    }

    /// A store holding every segment of `clips`' plan (a completed cold build).
    fn store_of(clips: &[ClipFacts], fade: usize) -> HashMap<String, u64> {
        let p = plan(clips, fade, &HashMap::new());
        p.segments
            .iter()
            .map(|s| (s.key.clone(), s.frames))
            .collect()
    }

    // Verifies: SR-037, LLR-055 (TC-076) — warm-unchanged partitions all
    // segments as hits; added-photos marks only the new clips plus their
    // transition neighbors as misses; removed-photo only the removed clip's
    // neighbors.
    #[test]
    fn plan_partitions_hits_and_misses_sr037() {
        let fade = 6;
        let base = album(&[("a", 24), ("b", 24), ("c", 24), ("d", 24), ("e", 24)]);
        let store = store_of(&base, fade);

        // Warm-unchanged: every segment is a hit, none re-encoded.
        let p = plan(&base, fade, &store);
        assert_eq!(p.segments.len(), 5, "one segment per clip");
        assert_eq!(p.misses(), 0, "warm-unchanged must re-encode nothing");

        // Added photo between c and d: the new clip and its two neighbors
        // (whose neighbor identity changed) miss; a/b/e stay hits.
        let mut added = base.clone();
        added.insert(3, clip("cx", 24, false));
        let p = plan(&added, fade, &store);
        let miss_names: Vec<&str> = p
            .segments
            .iter()
            .filter(|s| !s.hit)
            .map(|s| added[s.members.start].identity.path.as_str())
            .collect();
        assert_eq!(
            miss_names,
            vec!["c", "cx", "d"],
            "only the inserted clip plus its two neighbors re-encode"
        );

        // Removed photo c: only its former neighbors (b, d) miss.
        let mut removed = base.clone();
        removed.remove(2);
        let p = plan(&removed, fade, &store);
        let miss_names: Vec<&str> = p
            .segments
            .iter()
            .filter(|s| !s.hit)
            .map(|s| removed[s.members.start].identity.path.as_str())
            .collect();
        assert_eq!(
            miss_names,
            vec!["b", "d"],
            "only the removed clip's neighbors re-encode"
        );

        // Changed settings: every segment misses (full re-encode).
        let mut changed = params();
        changed.quality_crf = 23;
        let p = plan_segments(&base, fade, &changed, "t+f1", |k| {
            store.get(k).map(|&f| (PathBuf::from(format!("{k}.ts")), f))
        });
        assert_eq!(p.hits(), 0, "an encode-relevant change invalidates all");
    }

    // Verifies: SR-037, LLR-055 (TC-076) — segment start frames sit at
    // transition midpoints (second-half-in + body + first-half-out), frame
    // counts sum to the album total minus transition overlaps, and
    // sub-transition-length clips merge into their successor's segment.
    #[test]
    fn plan_start_frames_account_for_transition_overlap_sr037() {
        let fade = 6;
        // c=24, n=6: tail p=6, per-clip emitted 18, m=6 at each transition.
        let clips = album(&[("a", 24), ("b", 24), ("c", 24)]);
        let p = plan(&clips, fade, &HashMap::new());
        // Total = sum(c) - overlaps: overlap at each of 2 transitions is
        // p (6 tail frames merged into the blend) => 72 - 12 = 60.
        assert_eq!(p.layout.total, 60);
        // Boundaries at E_j + m/2: E_1=18 => 21; E_2=36 => 39.
        assert_eq!(
            p.segments.iter().map(|s| s.start_frame).collect::<Vec<_>>(),
            vec![0, 21, 39]
        );
        let sum: u64 = p.segments.iter().map(|s| s.frames).sum();
        assert_eq!(sum, p.layout.total, "segment frames partition the output");

        // Sub-transition clip (c=4 <= n): holds no tail, so no boundary
        // precedes its successor — it merges with the next clip's segment.
        let clips = album(&[("a", 24), ("tiny", 4), ("b", 24)]);
        let p = plan(&clips, fade, &HashMap::new());
        assert_eq!(p.segments.len(), 2, "tiny clip merges: 2 segments");
        assert_eq!(p.segments[1].members, 1..3, "tiny + successor share one");
        // tiny emits all 4 frames (m = min(6,4) = 4 blended), b emits 18,
        // final tail 6 => total = 18 + 4 + 18 + 6 = 46.
        assert_eq!(p.layout.total, 46);

        // Trailing sub-transition clip: no tail => no final fade-out frames.
        let clips = album(&[("a", 24), ("tiny", 4)]);
        let p = plan(&clips, fade, &HashMap::new());
        assert_eq!(p.layout.total, 18 + 4);
        assert_eq!(p.segments.len(), 2, "boundary exists into tiny (m=4)");
    }

    // Verifies: SR-037, SR-032, LLR-057 (TC-077) — for an album mixing images
    // and audio-bearing videos, the plan's audio start frames equal the
    // streaming mixer.emitted() capture (LLR-040) clip for clip, and the
    // plan's boundaries/total equal the mixer's actual rolls/emission — so
    // warm and cold builds mux identical adelay values via unchanged delay_ms.
    #[test]
    fn cached_audio_start_frames_match_streaming_capture_sr037() {
        // Frame-count permutations incl. sub-fade clips, asymmetric lengths,
        // and edge fade values; (album, fade) pairs.
        let cases: Vec<(Vec<u64>, usize)> = vec![
            (vec![24, 24, 24, 24], 6),
            (vec![24, 4, 24], 6),        // sub-fade middle clip
            (vec![4, 24, 24], 6),        // sub-fade first clip
            (vec![24, 24, 4], 6),        // sub-fade last clip
            (vec![10, 7, 13, 6, 30], 6), // between n and 2n mixtures
            (vec![24, 24], 0),           // fade disabled: one segment
            (vec![1, 1, 1], 3),          // degenerate one-frame clips
            (vec![30, 12, 5, 40, 8], 7),
        ];
        for (counts, fade) in cases {
            let clips: Vec<ClipFacts> = counts
                .iter()
                .enumerate()
                .map(|(i, &c)| clip(&format!("clip{i}"), c, i % 2 == 1))
                .collect();
            let p = plan(&clips, fade, &HashMap::new());

            // Ground truth: drive the real CrossfadeMixer over synthetic
            // frame sources with these counts, capturing emitted() before
            // each add_clip plus the roll frame indices.
            let probe = mixer_test_probe(&counts, fade);
            assert_eq!(
                p.layout.starts, probe.starts,
                "planned start frames must equal mixer.emitted() capture \
                 (counts {counts:?}, fade {fade})"
            );
            assert_eq!(
                p.layout.total, probe.total,
                "planned total must equal mixer emission (counts {counts:?})"
            );
            let planned_rolls: Vec<u64> = p.layout.boundaries.iter().map(|&(_, b)| b).collect();
            assert_eq!(
                planned_rolls, probe.rolls,
                "planned boundaries must equal actual mixer rolls \
                 (counts {counts:?}, fade {fade})"
            );

            // The audio clips inherit exactly those start frames.
            let audio = p.audio_clips(&clips);
            let expected: Vec<u64> = counts
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 1)
                .map(|(i, _)| probe.starts[i])
                .collect();
            assert_eq!(
                audio.iter().map(|a| a.start_frame).collect::<Vec<_>>(),
                expected
            );
        }
    }

    // Verifies: SR-037, LLR-055 — the video frame-count estimate is fps-scaled
    // duration, floored at 1 frame so the layout never sees a zero-length clip.
    #[test]
    fn estimate_video_frames_scales_duration_sr037() {
        assert_eq!(estimate_video_frames(Some(2.0), 30), 60);
        assert_eq!(estimate_video_frames(Some(0.0), 30), 1);
        assert_eq!(estimate_video_frames(None, 30), 1);
    }

    // Verifies: SR-037, LLR-053 — EncodeParams::of uses the even dims that
    // reach ffmpeg, so an odd configured size keys as its effective size.
    #[test]
    fn encode_params_use_even_dims_sr037() {
        let def = OutputDef {
            name: "t".into(),
            width: 1441,
            height: 901,
            fps: 30,
            pic_display_time_secs: 6.0,
            fade_time_secs: 0.5,
            max_rotation_degrees: 15.0,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            encoder: "software".into(),
            x264_preset: "medium".into(),
            zoom_amount: 0.12,
            ken_burns: true,
        };
        let p = EncodeParams::of(&def, "libx264", crate::transport::FrameTransport::Rgb24);
        assert_eq!((p.width, p.height), (1440, 900));
        assert_eq!(p.encoder, "libx264");
    }
}
