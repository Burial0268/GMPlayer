//! Sparse source-time anchors published only when a rendered block changes the timeline.

use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PcmPosition {
    pub playback_id: u64,
    pub seconds: f64,
    pub rate: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OutputAnchor {
    pub source: PcmPosition,
    pub transition: u64,
    /// 0: ordinary playback, 1: blending A, 2: B owns the output.
    pub phase: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderedAnchor {
    pub anchor: OutputAnchor,
    pub rendered_at: u64,
    pub version: u64,
}

#[derive(Default, Debug)]
pub(crate) struct RenderCounter {
    samples: AtomicU64,
    version: AtomicU64,
    playback_id: AtomicU64,
    seconds: AtomicU64,
    rate: AtomicU64,
    rendered_at: AtomicU64,
    transition: AtomicU64,
    phase: AtomicU64,
}

impl Deref for RenderCounter {
    type Target = AtomicU64;
    fn deref(&self) -> &Self::Target {
        &self.samples
    }
}

impl RenderCounter {
    /// Single writer: the device callback. No locks, allocation or destruction.
    pub(crate) fn publish(&self, anchor: OutputAnchor, rendered_at: u64) {
        self.version.fetch_add(1, Ordering::AcqRel);
        self.playback_id
            .store(anchor.source.playback_id, Ordering::Relaxed);
        self.seconds
            .store(anchor.source.seconds.to_bits(), Ordering::Relaxed);
        self.rate
            .store(anchor.source.rate.to_bits(), Ordering::Relaxed);
        self.rendered_at.store(rendered_at, Ordering::Relaxed);
        self.transition.store(anchor.transition, Ordering::Relaxed);
        self.phase.store(anchor.phase, Ordering::Relaxed);
        self.version.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn anchor(&self) -> Option<RenderedAnchor> {
        for _ in 0..3 {
            let version = self.version.load(Ordering::Acquire);
            if version == 0 || version & 1 != 0 {
                continue;
            }
            let result = RenderedAnchor {
                anchor: OutputAnchor {
                    source: PcmPosition {
                        playback_id: self.playback_id.load(Ordering::Relaxed),
                        seconds: f64::from_bits(self.seconds.load(Ordering::Relaxed)),
                        rate: f64::from_bits(self.rate.load(Ordering::Relaxed)),
                    },
                    transition: self.transition.load(Ordering::Relaxed),
                    phase: self.phase.load(Ordering::Relaxed),
                },
                rendered_at: self.rendered_at.load(Ordering::Relaxed),
                version,
            };
            std::sync::atomic::fence(Ordering::Acquire);
            if self.version.load(Ordering::Relaxed) == version {
                return Some(result);
            }
        }
        None
    }
}
