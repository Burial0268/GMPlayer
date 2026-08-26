use crate::output::OutputRenderClock;

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
    }

    pub(super) fn set_duration(&mut self, duration: f64) {
        self.duration = duration.max(0.0);
        self.base_position = self.clamp_position(self.base_position);
    }

    pub(super) fn set_anchor(&mut self, is_playing: bool, position: f64) -> f64 {
        let position = self.clamp_position(position);
        self.base_position = position;
        self.base_rendered_samples = self.rendered_samples();
        self.is_playing = is_playing;
        position
    }

    pub(super) fn position(&self) -> f64 {
        let position = if self.is_playing {
            let rendered_delta = self
                .rendered_samples()
                .saturating_sub(self.base_rendered_samples);
            let samples_per_second = self.sample_rate.max(1) as f64 * self.channels.max(1) as f64;
            self.base_position + rendered_delta as f64 / samples_per_second
        } else {
            self.base_position
        };
        self.clamp_position(position)
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
