use crate::output::{OutputRenderClock, RenderedAnchor};
use std::cell::Cell;

#[derive(Debug)]
pub(super) struct PlayerClock {
    base_position: f64,
    base_rendered_samples: u64,
    is_playing: bool,
    duration: f64,
    render_clock: Option<OutputRenderClock>,
    sample_rate: u32,
    channels: u16,
    epoch: u64,
    playback_id: u64,
    base_rate: f64,
    anchor_version: u64,
    last_source_anchor: Cell<Option<RenderedAnchor>>,
}

impl PlayerClock {
    pub(super) fn new() -> Self {
        Self {
            base_position: 0.0,
            base_rendered_samples: 0,
            is_playing: false,
            duration: 0.0,
            render_clock: None,
            sample_rate: 44_100,
            channels: 2,
            epoch: 0,
            playback_id: 0,
            base_rate: 1.0,
            anchor_version: 0,
            last_source_anchor: Cell::new(None),
        }
    }

    pub(super) fn set_render_clock(
        &mut self,
        render_clock: OutputRenderClock,
        sample_rate: u32,
        channels: u16,
    ) {
        let position = self.position();
        self.render_clock = Some(render_clock);
        self.sample_rate = sample_rate.max(1);
        self.channels = channels.max(1);
        self.base_position = self.clamp_position(position);
        self.base_rendered_samples = self.rendered_samples();
        self.anchor_version = 0;
        self.last_source_anchor.set(None);
    }

    pub(super) fn set_playback_id(&mut self, playback_id: u64) {
        self.playback_id = playback_id;
        self.base_rate = 1.0;
        self.anchor_version = 0;
        self.last_source_anchor.set(None);
    }

    pub(super) fn reset_rate(&mut self) {
        self.base_rate = 1.0;
        self.anchor_version = self
            .render_clock
            .as_ref()
            .and_then(OutputRenderClock::anchor)
            .map_or(0, |anchor| anchor.version);
    }

    pub(super) fn set_duration(&mut self, duration: f64) {
        self.duration = duration.max(0.0);
        self.base_position = self.clamp_position(self.base_position);
    }

    pub(super) fn set_anchor(&mut self, is_playing: bool, position: f64) -> f64 {
        let position = self.clamp_position(position);
        self.base_rate = self.mapping().2;
        self.anchor_version = self
            .render_clock
            .as_ref()
            .and_then(OutputRenderClock::anchor)
            .map_or(0, |anchor| anchor.version);
        self.base_position = position;
        self.base_rendered_samples = self.rendered_samples();
        self.is_playing = is_playing;
        position
    }

    pub(super) fn position(&self) -> f64 {
        let position = if self.is_playing {
            let (base_position, base_rendered, rate) = self.mapping();
            let rendered_delta = self.rendered_samples().saturating_sub(base_rendered);
            let samples_per_second = self.sample_rate.max(1) as f64 * self.channels.max(1) as f64;
            base_position + rendered_delta as f64 / samples_per_second * rate
        } else {
            self.base_position
        };
        self.clamp_position(position)
    }

    /// Measured delay between the position this clock reports and what is
    /// audible, in microseconds; `0` until a measurement settles. Settles a new
    /// one, so call it from the heartbeat, not per frame.
    pub(super) fn settle_output_latency_us(&self) -> u32 {
        self.render_clock
            .as_ref()
            .map_or(0, OutputRenderClock::settle_latency_us)
    }

    pub(super) fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Which timeline the current anchor belongs to.
    ///
    /// Position packets are otherwise anonymous, and a subscriber cannot tell a
    /// fresh track's `0.0` from a stale packet describing the track it replaced —
    /// both are "a position behind the one I hold". Guessing from the magnitude
    /// is what made the frontend read the previous track's elapsed time plus the
    /// new one's: it rejected every anchor the new track published as a rewind
    /// nothing had asked for, and kept extrapolating the retired timeline.
    ///
    /// So the clock stamps its epoch on everything it publishes and the
    /// subscriber compares that instead. It is monotonic and process-local; it
    /// deliberately says nothing about *which* track, only that the timeline it
    /// belongs to is not the previous one — identity answers the "which".
    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Start a new timeline. Called only where playback genuinely restarts from
    /// a new source — a track load, a native AutoMix hand-off, the retirement of
    /// the last track — and never for seek, pause/resume or an output rebuild,
    /// all of which keep the same track's timeline.
    pub(super) fn begin_epoch(&mut self) -> u64 {
        self.epoch = self.epoch.wrapping_add(1);
        self.epoch
    }

    fn mapping(&self) -> (f64, u64, f64) {
        if let Some(anchor) = self
            .render_clock
            .as_ref()
            .and_then(OutputRenderClock::anchor)
        {
            if anchor.anchor.source.playback_id == self.playback_id
                && anchor.version > self.anchor_version
                && anchor.rendered_at >= self.base_rendered_samples
                && anchor.rendered_at <= self.rendered_samples()
            {
                self.last_source_anchor.set(Some(anchor));
                return (
                    anchor.anchor.source.seconds,
                    anchor.rendered_at,
                    anchor.anchor.source.rate,
                );
            }
        }
        // B's marker can render before the player loop adopts its identity.
        // Keep A's last mapping for that short interval rather than stepping back.
        if let Some(anchor) = self.last_source_anchor.get() {
            if anchor.anchor.source.playback_id == self.playback_id
                && anchor.version > self.anchor_version
                && anchor.rendered_at >= self.base_rendered_samples
            {
                return (
                    anchor.anchor.source.seconds,
                    anchor.rendered_at,
                    anchor.anchor.source.rate,
                );
            }
        }
        (
            self.base_position,
            self.base_rendered_samples,
            self.base_rate,
        )
    }

    fn clamp_position(&self, position: f64) -> f64 {
        let position = normalize_seek_position(position);
        if self.duration > 0.0 {
            position.min(self.duration)
        } else {
            position
        }
    }

    fn rendered_samples(&self) -> u64 {
        self.render_clock
            .as_ref()
            .map(OutputRenderClock::rendered_samples)
            .unwrap_or(self.base_rendered_samples)
    }
}

pub(super) fn normalize_seek_position(position: f64) -> f64 {
    if position.is_finite() {
        position.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the epoch: everything that continues the *same*
    /// track's timeline must leave it alone, so a subscriber can treat "same
    /// epoch" as "the guards apply" without enumerating the paths itself.
    #[test]
    fn an_incoming_marker_does_not_rewind_the_outgoing_clock_before_adoption() {
        use crate::output::{test_support::TestOutput, OutputAnchor, PcmPosition, PushCancel};
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let mut output = TestOutput::new(1, 48_000, 4);
        let stop = AtomicBool::new(false);
        let interrupt = AtomicU64::new(0);
        let cancel = || PushCancel::with_interrupt_epoch(&stop, &interrupt, 0);
        let mut clock = PlayerClock::new();
        clock.set_render_clock(output.writer.render_clock(), 48_000, 1);
        clock.set_playback_id(1);
        clock.set_anchor(true, 10.0);
        let a = OutputAnchor {
            source: PcmPosition {
                playback_id: 1,
                seconds: 10.0,
                rate: 1.04,
            },
            transition: 9,
            phase: 1,
        };
        output
            .writer
            .push_block_with_anchor(vec![0.1; 48_000], Some(a), cancel());
        output.render(48_000);
        let before = clock.position();
        assert!((before - 11.04).abs() < 1e-9);
        let b = OutputAnchor {
            source: PcmPosition {
                playback_id: 2,
                seconds: 0.2,
                rate: 1.0,
            },
            transition: 9,
            phase: 2,
        };
        output
            .writer
            .push_block_with_anchor(vec![0.1; 256], Some(b), cancel());
        output.render(256);
        assert!(clock.position() > before);
        clock.set_playback_id(2);
        clock.begin_epoch();
        let adopted = 0.2 + 256.0 / 48_000.0;
        clock.set_anchor(true, adopted);
        assert!((clock.position() - adopted).abs() < 1e-9);
    }

    #[test]
    fn rendered_source_rate_survives_pause_and_a_new_track_reanchors() {
        use crate::output::{test_support::TestOutput, OutputAnchor, PcmPosition, PushCancel};
        use std::sync::atomic::AtomicBool;
        let mut output = TestOutput::new(1, 48_000, 4);
        let stop = AtomicBool::new(false);
        let interrupt = std::sync::atomic::AtomicU64::new(0);
        let cancel = || PushCancel::with_interrupt_epoch(&stop, &interrupt, 0);
        let mut clock = PlayerClock::new();
        clock.set_render_clock(output.writer.render_clock(), 48_000, 1);
        clock.set_playback_id(17);
        clock.set_duration(100.0);
        clock.set_anchor(true, 10.0);
        let anchor = OutputAnchor {
            source: PcmPosition {
                playback_id: 17,
                seconds: 10.0,
                rate: 1.04,
            },
            transition: 7,
            phase: 1,
        };
        assert!(output
            .writer
            .push_block_with_anchor(vec![0.1; 48_000], Some(anchor), cancel()));
        output.render(24_000);
        assert!((clock.position() - 10.52).abs() < 1e-9);
        let at_pause = clock.position();
        output.writer.set_paused(true);
        clock.set_anchor(false, at_pause);
        output.render(24_000);
        assert_eq!(clock.position(), at_pause);
        clock.set_anchor(true, at_pause);
        output.writer.set_paused(false);
        output.render(24_000);
        assert!((clock.position() - 11.04).abs() < 1e-9);
        clock.set_playback_id(18);
        clock.begin_epoch();
        clock.set_anchor(true, 0.2);
        let anchor = OutputAnchor {
            source: PcmPosition {
                playback_id: 18,
                seconds: 0.2,
                rate: 1.0,
            },
            transition: 7,
            phase: 2,
        };
        assert!(output
            .writer
            .push_block_with_anchor(vec![0.1; 24_000], Some(anchor), cancel()));
        output.render(24_000);
        assert!((clock.position() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn seek_pause_and_resume_stay_on_one_timeline() {
        let mut clock = PlayerClock::new();
        clock.set_duration(200.0);
        let epoch = clock.begin_epoch();

        clock.set_anchor(true, 0.0);
        clock.set_anchor(false, 30.0); // pause
        clock.set_anchor(true, 30.0); // resume
        clock.set_anchor(true, 120.0); // seek
        clock.set_duration(200.0); // output rebuild republishes duration

        assert_eq!(clock.epoch(), epoch);
    }

    #[test]
    fn a_new_source_retires_the_previous_timeline() {
        let mut clock = PlayerClock::new();
        let first = clock.begin_epoch();
        let second = clock.begin_epoch();
        assert_ne!(first, second);
        assert_eq!(clock.epoch(), second);
    }

    /// A fresh track's anchor is *behind* the one it replaced, which is exactly
    /// what a stale packet looks like. Only the epoch separates them.
    #[test]
    fn a_fresh_track_anchors_behind_the_one_it_replaced() {
        let mut clock = PlayerClock::new();
        clock.set_duration(200.0);
        let previous = clock.begin_epoch();
        let retired_position = clock.set_anchor(true, 180.0);

        clock.set_duration(240.0);
        let current = clock.begin_epoch();
        let fresh_position = clock.set_anchor(true, 0.0);

        assert!(fresh_position < retired_position);
        assert_ne!(current, previous);
    }
}
