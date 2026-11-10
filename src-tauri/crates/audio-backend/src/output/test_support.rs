//! Deterministic device callback harness; no physical audio device is opened.

use super::*;

pub(crate) struct TestOutput {
    pub writer: OutputWriter,
    state: CallbackState,
}

impl TestOutput {
    pub(crate) fn new(channels: u16, sample_rate: u32, capacity: usize) -> Self {
        let (producer, consumer) = RingBuffer::new(capacity);
        let (recycle_tx, recycle_rx) = RingBuffer::new(capacity + 4);
        let counter = Arc::new(RenderCounter::default());
        let alive = Arc::new(AtomicBool::new(true));
        let writer = OutputWriter {
            data_tx: Arc::new(Mutex::new(producer)),
            paused: Arc::new(AtomicBool::new(false)),
            stream_failed: Arc::new(AtomicBool::new(false)),
            callback_alive: Arc::clone(&alive),
            volume_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            generation: Arc::new(AtomicU64::new(0)),
            flush_epoch: Arc::new(AtomicU64::new(0)),
            queued_samples: Arc::new(AtomicUsize::new(0)),
            rendered_samples: Arc::clone(&counter),
            latency: Arc::new(LatencyMeter::new()),
            samples_per_sec: stream_samples_per_sec(sample_rate, channels),
            recycle_rx: Arc::new(Mutex::new(recycle_rx)),
        };
        let mut state = CallbackState::new(
            consumer,
            recycle_tx,
            alive,
            0,
            0,
            channels as usize,
            capacity + 4,
        );
        state.set_render_counter(counter);
        Self { writer, state }
    }

    pub(crate) fn render(&mut self, samples: usize) -> Vec<f32> {
        let mut data = vec![0.0; samples];
        fill_output(
            &mut data,
            &mut self.state,
            &self.writer.paused,
            &self.writer.volume_bits,
            &self.writer.generation,
            &self.writer.flush_epoch,
            &self.writer.queued_samples,
            &self.writer.rendered_samples,
        );
        data
    }
}
