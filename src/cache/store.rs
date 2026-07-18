//! Segment store (SR-037): on-disk cache of encoded MPEG-TS segments plus a
//! JSON index (key → bytes, frames, last-used), size-capped by LRU pruning.
//! A corrupt/unreadable entry or index is a miss with quiet eviction — never a
//! build failure. Rooted at `ProcessingConfig.temp_dir` when set, else the
//! per-user cache dir (`util::file_utils::app_cache_root`, the LLR-031 root).

use super::{clip_frames_key, SourceIdentity};
use crate::error::{Result, SlideshowError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Milliseconds after which an unused per-clip frame-count record is dropped
/// during pruning (segments themselves are pruned by the byte cap; these tiny
/// records just need an eventual horizon).
const CLIP_FRAMES_TTL_MS: u64 = 90 * 24 * 3600 * 1000;

/// One cached segment's index row.
// Implements: LLR-054, SR-037
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMeta {
    /// Expected byte size of `<key>.ts` (validated on lookup).
    pub bytes: u64,
    /// Output frames the segment contains (drives the LLR-057 timeline).
    pub frames: u64,
    /// Last time the entry was inserted or served (epoch ms) — the LRU order.
    pub last_used_ms: u64,
}

/// A recorded actual source-frame count for one clip identity at one fps
/// (videos; images are computed). Keeps warm-plan arithmetic exact (LLR-055).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClipFramesMeta {
    frames: u64,
    last_used_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    segments: BTreeMap<String, SegmentMeta>,
    clip_frames: BTreeMap<String, ClipFramesMeta>,
}

/// The per-user (or `temp_dir`-rooted) segment store. All mutation happens in
/// memory until [`SegmentStore::save`] rewrites the index atomically.
// Implements: LLR-054, SR-037
pub struct SegmentStore {
    root: PathBuf,
    index: Index,
}

impl SegmentStore {
    /// The store directory for a configured `temp_dir` (when set) or the
    /// per-user cache root: `<root>/segment-cache`. Always a dedicated
    /// subdirectory so `--clear-cache` can never touch unrelated files.
    // Implements: LLR-054, SR-037
    pub fn root_for(temp_dir: Option<&Path>) -> PathBuf {
        temp_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(crate::util::file_utils::app_cache_root)
            .join("segment-cache")
    }

    /// Open (creating if missing) the store at `root`. A missing or corrupt
    /// index yields an empty index; any `.ts` file not in the index and any
    /// index row without a valid file is quietly evicted (SR-037
    /// corrupted-cache-entry: a miss, never a failure). Stale staging dirs
    /// from killed runs are swept too.
    // Implements: LLR-054, SR-037
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).map_err(|e| {
            SlideshowError::Processing(format!(
                "cannot create segment cache dir {}: {}",
                root.display(),
                e
            ))
        })?;
        let index_path = root.join("index.json");
        let mut index: Index = std::fs::read_to_string(&index_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();

        // Reconcile index vs directory, both directions, quietly.
        index.segments.retain(|key, meta| {
            let ok = std::fs::metadata(root.join(format!("{key}.ts")))
                .map(|m| m.len() == meta.bytes && m.len() > 0)
                .unwrap_or(false);
            if !ok {
                let _ = std::fs::remove_file(root.join(format!("{key}.ts")));
            }
            ok
        });
        // Sweep leftovers from killed runs — but only when old enough that no
        // live build can own them: another process may share this store (its
        // staging dirs and just-committed segments are not in OUR index yet),
        // so a fresh artifact is never touched.
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if !is_stale(&p) {
                    continue;
                }
                if p.is_dir() && name.starts_with("staging-") {
                    let _ = std::fs::remove_dir_all(&p);
                } else if let Some(stem) = name.strip_suffix(".ts") {
                    if !index.segments.contains_key(stem) {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            index,
        })
    }

    /// On-disk path for `key`'s segment.
    pub fn segment_path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.ts"))
    }

    /// A fresh staging directory for one encode run (same volume as the
    /// store, so committing is an atomic rename). Swept on `open` if a killed
    /// run leaves it behind.
    pub fn staging_dir(&self) -> Result<PathBuf> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = self.root.join(format!(
            "staging-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| {
            SlideshowError::Processing(format!(
                "cannot create segment staging dir {}: {}",
                dir.display(),
                e
            ))
        })?;
        Ok(dir)
    }

    /// Serve `key` if present AND valid (file exists, non-zero, size matches
    /// the index); returns the segment path and its frame count, bumping the
    /// LRU clock. An invalid entry is quietly evicted and reported as a miss.
    // Implements: LLR-054, SR-037
    pub fn lookup(&mut self, key: &str) -> Option<(PathBuf, u64)> {
        let path = self.segment_path(key);
        let meta = self.index.segments.get_mut(key)?;
        let valid = std::fs::metadata(&path)
            .map(|m| m.len() == meta.bytes && m.len() > 0)
            .unwrap_or(false);
        if !valid {
            // Corrupt/truncated/missing: miss + quiet eviction (SR-037).
            self.index.segments.remove(key);
            let _ = std::fs::remove_file(&path);
            return None;
        }
        meta.last_used_ms = now_ms();
        let frames = meta.frames;
        Some((path, frames))
    }

    /// Move a freshly-encoded segment from its staging path into the store
    /// under `key`, recording its size/frames and bumping the LRU clock.
    // Implements: LLR-054, SR-037
    pub fn commit(&mut self, key: &str, staged: &Path, frames: u64) -> Result<PathBuf> {
        let dest = self.segment_path(key);
        let _ = std::fs::remove_file(&dest);
        std::fs::rename(staged, &dest).map_err(|e| {
            SlideshowError::Processing(format!("cannot store segment {}: {}", dest.display(), e))
        })?;
        let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        self.index.segments.insert(
            key.to_string(),
            SegmentMeta {
                bytes,
                frames,
                last_used_ms: now_ms(),
            },
        );
        Ok(dest)
    }

    /// Record a clip's actual source frame count (video decode at one fps).
    // Implements: LLR-055, SR-037
    pub fn record_clip_frames(&mut self, id: &SourceIdentity, fps: u32, frames: u64) {
        self.index.clip_frames.insert(
            clip_frames_key(id, fps),
            ClipFramesMeta {
                frames,
                last_used_ms: now_ms(),
            },
        );
    }

    /// A clip's recorded actual frame count, if previously encoded.
    // Implements: LLR-055, SR-037
    pub fn clip_frames(&mut self, id: &SourceIdentity, fps: u32) -> Option<u64> {
        let meta = self.index.clip_frames.get_mut(&clip_frames_key(id, fps))?;
        meta.last_used_ms = now_ms();
        Some(meta.frames)
    }

    /// Total bytes of all indexed segments.
    pub fn total_bytes(&self) -> u64 {
        self.index.segments.values().map(|m| m.bytes).sum()
    }

    /// Evict least-recently-used segments until the store holds at most
    /// `cap_bytes`, and drop clip-frame records unused past their TTL.
    // Implements: LLR-054, SR-037
    pub fn prune_to(&mut self, cap_bytes: u64) {
        let mut by_age: Vec<(String, u64, u64)> = self
            .index
            .segments
            .iter()
            .map(|(k, m)| (k.clone(), m.last_used_ms, m.bytes))
            .collect();
        by_age.sort_by_key(|&(_, last_used, _)| last_used);
        let mut total = self.total_bytes();
        for (key, _, bytes) in by_age {
            if total <= cap_bytes {
                break;
            }
            self.index.segments.remove(&key);
            let _ = std::fs::remove_file(self.segment_path(&key));
            total -= bytes;
        }
        let horizon = now_ms().saturating_sub(CLIP_FRAMES_TTL_MS);
        self.index
            .clip_frames
            .retain(|_, m| m.last_used_ms >= horizon);
    }

    /// Empty the store: remove every segment, the index file, and all
    /// in-memory state (`--clear-cache`).
    // Implements: LLR-054, SR-037
    pub fn clear(&mut self) {
        for key in self.index.segments.keys().cloned().collect::<Vec<_>>() {
            let _ = std::fs::remove_file(self.segment_path(&key));
        }
        let _ = std::fs::remove_file(self.root.join("index.json"));
        self.index = Index::default();
    }

    /// Atomically persist the index (`index.json.tmp` → rename), mirroring the
    /// probe cache: a killed save leaves the old index, never a corrupt one.
    // Implements: LLR-054, SR-037
    pub fn save(&self) -> Result<()> {
        let path = self.root.join("index.json");
        let tmp = self.root.join("index.json.tmp");
        let body = serde_json::to_string(&self.index)
            .map_err(|e| SlideshowError::Processing(format!("segment index serialize: {}", e)))?;
        std::fs::write(&tmp, body)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                SlideshowError::Processing(format!(
                    "cannot write segment index {}: {}",
                    path.display(),
                    e
                ))
            })
    }
}

/// Whether a leftover artifact is old enough (24h) to be from a dead run and
/// safe to sweep. An unreadable mtime is treated as fresh (never swept) so a
/// racing live build is never damaged.
fn is_stale(path: &Path) -> bool {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age > STALE_AFTER)
        .unwrap_or(false)
}

/// Epoch milliseconds for LRU bookkeeping (0 before the epoch — ordering only,
/// never correctness).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "slideshow_segstore_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn stage(store: &SegmentStore, name: &str, bytes: &[u8]) -> PathBuf {
        let dir = store.staging_dir().unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    // Verifies: SR-037, LLR-054 (TC-078, round-trip leg) — insert/lookup
    // round-trips a segment with its frame count and survives reopen.
    #[test]
    fn store_roundtrips_and_persists_sr037() {
        let root = temp_root("rt");
        let mut s = SegmentStore::open(&root).unwrap();
        let staged = stage(&s, "a.ts", b"segment-bytes");
        s.commit("k1", &staged, 42).unwrap();
        assert_eq!(s.lookup("k1").map(|(_, f)| f), Some(42));
        assert!(s.lookup("absent").is_none());
        s.save().unwrap();

        let mut reopened = SegmentStore::open(&root).unwrap();
        let (path, frames) = reopened.lookup("k1").expect("persisted entry");
        assert_eq!(frames, 42);
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    // Verifies: SR-037, LLR-054 (TC-078, corrupt-entry leg) — a truncated or
    // missing segment file, and a corrupt index, are each treated as a miss
    // with the bad entry quietly deleted; the caller never fails.
    #[test]
    fn corrupt_entry_is_miss_and_deleted_sr037() {
        let root = temp_root("corrupt");
        let mut s = SegmentStore::open(&root).unwrap();
        s.commit("trunc", &stage(&s, "t.ts", b"0123456789"), 5)
            .unwrap();
        s.commit("gone", &stage(&s, "g.ts", b"0123456789"), 5)
            .unwrap();
        s.save().unwrap();

        // Truncate one file, remove the other behind the store's back.
        std::fs::write(s.segment_path("trunc"), b"012").unwrap();
        std::fs::remove_file(s.segment_path("gone")).unwrap();
        assert!(s.lookup("trunc").is_none(), "truncated entry is a miss");
        assert!(!s.segment_path("trunc").exists(), "quietly evicted");
        assert!(s.lookup("gone").is_none(), "missing file is a miss");

        // Corrupt index: open starts empty (every entry a miss, never an
        // error). A FRESH orphan .ts is left alone — it may belong to a live
        // build sharing the store — and only becomes sweepable after the
        // staleness horizon (is_stale).
        std::fs::write(root.join("index.json"), b"{not json").unwrap();
        std::fs::write(root.join("orphan.ts"), b"stale").unwrap();
        let mut s = SegmentStore::open(&root).unwrap();
        assert!(s.lookup("trunc").is_none());
        assert!(
            root.join("orphan.ts").exists(),
            "fresh orphan preserved (racing live build safety)"
        );
        assert!(
            !is_stale(&root.join("orphan.ts")),
            "a just-written file is not stale"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Verifies: SR-037, LLR-054 (TC-078, prune leg) — prune_to evicts
    // least-recently-used entries down to the byte cap.
    #[test]
    fn store_prunes_lru_to_cap_sr037() {
        let root = temp_root("lru");
        let mut s = SegmentStore::open(&root).unwrap();
        for (i, key) in ["old", "mid", "new"].iter().enumerate() {
            let staged = stage(&s, &format!("{key}.ts"), &[0u8; 100]);
            s.commit(key, &staged, 10).unwrap();
            // Force distinct LRU stamps (ms clock granularity).
            s.index.segments.get_mut(*key).unwrap().last_used_ms = i as u64 + 1;
        }
        assert_eq!(s.total_bytes(), 300);
        s.prune_to(250);
        assert!(s.lookup("old").is_none(), "LRU entry evicted first");
        assert!(s.lookup("mid").is_some());
        assert!(s.lookup("new").is_some());
        assert!(s.total_bytes() <= 250);
        let _ = std::fs::remove_dir_all(&root);
    }

    // Verifies: SR-037, LLR-054 (TC-078, clear + root legs) — clear() empties
    // the store; root_for prefers temp_dir and always appends the dedicated
    // subdirectory.
    #[test]
    fn clear_empties_and_root_prefers_temp_dir_sr037() {
        let root = temp_root("clear");
        let mut s = SegmentStore::open(&root).unwrap();
        s.commit("k", &stage(&s, "k.ts", b"x"), 1).unwrap();
        s.clear();
        assert!(s.lookup("k").is_none());
        assert!(!s.segment_path("k").exists());

        let t = PathBuf::from("R:/scratch");
        assert_eq!(SegmentStore::root_for(Some(&t)), t.join("segment-cache"));
        assert_eq!(
            SegmentStore::root_for(None),
            crate::util::file_utils::app_cache_root().join("segment-cache")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Verifies: SR-037, LLR-055 — clip frame records round-trip and are
    // dropped past the TTL horizon during pruning.
    #[test]
    fn clip_frames_recorded_and_expired_sr037() {
        let root = temp_root("cf");
        let mut s = SegmentStore::open(&root).unwrap();
        let id = SourceIdentity {
            path: "v.mp4".into(),
            size: 1,
            mtime_ms: 2,
        };
        s.record_clip_frames(&id, 30, 77);
        assert_eq!(s.clip_frames(&id, 30), Some(77));
        assert_eq!(s.clip_frames(&id, 24), None, "fps is part of the identity");

        // Age the record past the TTL and prune.
        for m in s.index.clip_frames.values_mut() {
            m.last_used_ms = 1;
        }
        s.prune_to(u64::MAX);
        assert_eq!(s.clip_frames(&id, 30), None, "expired record dropped");
        let _ = std::fs::remove_dir_all(&root);
    }
}
