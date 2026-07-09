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
