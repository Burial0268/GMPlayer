//! One-ahead resolved-source cache.
//!
//! Holds at most one prepared source (the planner's next track) plus the
//! source backing the currently playing track. Keeping it to one-ahead is
//! deliberate: it bounds concurrent downloads, temp-file disk use and memory
//! regardless of how long the playlist is.
//!
//! Every prefetch carries the `(manifest_revision, generation)` it was started
//! under. A result whose generation no longer matches is dropped on arrival,
//! which is what makes list edits, clears and manual skips safe while a
//! resolve is in flight.

use crate::types::TrackIdentity;

use super::source_resolver::{ResolveError, ResolvedSource};

/// Outcome of a prefetch task, delivered back to the player loop.
#[derive(Debug)]
pub struct PrefetchResult {
    pub generation: u64,
    pub manifest_revision: u64,
    /// Position in the manifest this prefetch was for — used by the planner's
    /// failure bookkeeping.
    pub position: usize,
    pub outcome: Result<ResolvedSource, ResolveError>,
}

#[derive(Debug, Default)]
pub struct SourceCache {
    prepared: Option<PreparedSource>,
    /// Generation of the newest issued prefetch. Results carrying anything
    /// older are stale and must be discarded.
    generation: u64,
    /// True while a prefetch for the current generation is outstanding.
    in_flight: bool,
}

#[derive(Debug)]
struct PreparedSource {
    position: usize,
    source: ResolvedSource,
}

impl SourceCache {
    pub fn new() -> Self {
        Self {
            prepared: None,
            generation: 0,
            in_flight: false,
        }
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    /// Whether the prepared source (if any) belongs to `position`.
    #[cfg(test)]
    pub fn prepared_position(&self) -> Option<usize> {
        self.prepared.as_ref().map(|p| p.position)
    }

    /// Invalidate everything in flight and drop the prepared source. Called on
    /// manifest replace/clear, manual track changes and planner gating.
    pub fn invalidate(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.prepared = None;
        self.in_flight = false;
        self.generation
    }

    /// Begin a prefetch. Returns the generation to stamp the task with.
    pub fn begin_prefetch(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = true;
        self.generation
    }

    /// Accept a prefetch result. Returns `false` when it belongs to a
    /// superseded generation and was discarded.
    pub fn accept(&mut self, result: PrefetchResult) -> bool {
        if result.generation != self.generation {
            // Manifest replacement invalidates the cache, so a stale revision
            // always shows up as a stale generation too — logged to make that
            // relationship visible when diagnosing a skipped prefetch.
            log::debug!(
                "dropping prefetch from superseded generation: revision={} got={} want={}",
                result.manifest_revision,
                result.generation,
                self.generation
            );
            return false;
        }
        self.in_flight = false;
        match result.outcome {
            Ok(source) => {
                self.prepared = Some(PreparedSource {
                    position: result.position,
                    source,
                });
                true
            }
            Err(_) => {
                self.prepared = None;
                true
            }
        }
    }

    /// Whether a usable (non-expired) source is prepared for `position`.
    pub fn has_fresh_for(&self, position: usize) -> bool {
        self.prepared
            .as_ref()
            .is_some_and(|prepared| prepared.position == position && !prepared.source.is_stale())
    }

    pub fn prepared_identity(&self) -> Option<&TrackIdentity> {
        self.prepared.as_ref().map(|p| &p.source.identity)
    }

    /// Take the prepared source if it matches `position` and is still fresh.
    /// A stale entry is dropped rather than returned — the caller re-resolves.
    pub fn take_for(&mut self, position: usize) -> Option<ResolvedSource> {
        let prepared = self.prepared.as_ref()?;
        if prepared.position != position {
            return None;
        }
        if prepared.source.is_stale() {
            self.prepared = None;
            return None;
        }
        self.prepared.take().map(|prepared| prepared.source)
    }
}

#[cfg(test)]
mod tests {
    use super::super::source_resolver::{ResolveErrorKind, SourceOrigin};
    use super::*;

    fn identity(id: &str) -> TrackIdentity {
        TrackIdentity::Netease { id: id.to_string() }
    }

    fn source(id: &str) -> ResolvedSource {
        ResolvedSource::remote(
            identity(id),
            format!("https://cdn.example.com/{id}.mp3"),
            SourceOrigin::Ncm,
        )
    }

    fn ok_result(generation: u64, position: usize, id: &str) -> PrefetchResult {
        PrefetchResult {
            generation,
            manifest_revision: 1,
            position,
            outcome: Ok(source(id)),
        }
    }

    #[test]
    fn accepts_a_current_generation_result() {
        let mut cache = SourceCache::new();
        let generation = cache.begin_prefetch();
        assert!(cache.is_in_flight());

        assert!(cache.accept(ok_result(generation, 4, "a")));
        assert!(!cache.is_in_flight());
        assert!(cache.has_fresh_for(4));
        assert_eq!(cache.prepared_position(), Some(4));
    }

    #[test]
    fn discards_results_from_a_superseded_generation() {
        let mut cache = SourceCache::new();
        let stale_generation = cache.begin_prefetch();
        // A list edit lands while the resolve is in flight.
        cache.invalidate();

        assert!(
            !cache.accept(ok_result(stale_generation, 4, "a")),
            "a result from before the edit must not be adopted"
        );
        assert!(cache.prepared_identity().is_none());
    }

    #[test]
    fn invalidate_drops_a_prepared_source() {
        let mut cache = SourceCache::new();
        let generation = cache.begin_prefetch();
        cache.accept(ok_result(generation, 2, "a"));
        assert!(cache.has_fresh_for(2));

        cache.invalidate();
        assert!(!cache.has_fresh_for(2));
        assert!(cache.take_for(2).is_none());
    }

    #[test]
    fn take_only_returns_the_matching_position() {
        let mut cache = SourceCache::new();
        let generation = cache.begin_prefetch();
        cache.accept(ok_result(generation, 7, "a"));

        assert!(cache.take_for(8).is_none(), "wrong position must not match");
        let taken = cache.take_for(7).expect("matching position");
        assert_eq!(taken.identity.key(), "netease:a");
        assert!(cache.take_for(7).is_none(), "take must consume");
    }

    #[test]
    fn a_failed_prefetch_clears_in_flight_without_preparing() {
        let mut cache = SourceCache::new();
        let generation = cache.begin_prefetch();

        assert!(cache.accept(PrefetchResult {
            generation,
            manifest_revision: 1,
            position: 3,
            outcome: Err(ResolveError {
                kind: ResolveErrorKind::Unavailable,
                message: "gone".into(),
            }),
        }));
        assert!(!cache.is_in_flight());
        assert!(!cache.has_fresh_for(3));
    }

    #[test]
    fn each_prefetch_supersedes_the_previous_one() {
        let mut cache = SourceCache::new();
        let first = cache.begin_prefetch();
        let second = cache.begin_prefetch();
        assert_ne!(first, second);

        assert!(!cache.accept(ok_result(first, 1, "a")));
        assert!(cache.accept(ok_result(second, 2, "b")));
        assert_eq!(cache.prepared_position(), Some(2));
    }
}
