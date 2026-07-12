use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::{FromSample, Sample};
use rtrb::{Consumer, Producer, PushError};

const UNDERRUN_FADE_FRAMES: usize = 64;
const SEEK_FLUSH_FADE_FRAMES: usize = 256;

pub(super) struct OutputBlock {
    pub(super) samples: Vec<f32>,
    pub(super) generation: u64,
    pub(super) flush_epoch: u64,
}

pub(super) struct CallbackState {
    data_rx: Consumer<OutputBlock>,
    /// Returns spent mix blocks to the mixer through a wait-free SPSC ring.
    /// A full ring returns ownership to the callback for bounded retention, so
    /// this path neither blocks nor destroys the allocation.
    recycle_tx: Producer<Vec<f32>>,
    callback_alive: Arc<AtomicBool>,
    /// Spent blocks that could not be returned because the bounded recycle
    /// queue was temporarily full. Capacity is reserved before the stream is
    /// started, so retaining buffers here never allocates in the callback.
    /// Multiple slots let PCM consumption continue through short producer
    /// stalls instead of turning recycle backpressure into an underrun.
    pending_recycles: Vec<Vec<f32>>,
    current_block: Vec<f32>,
    current_index: usize,
    accepted_generation: u64,
    accepted_flush_epoch: u64,
    output_channels: usize,
    last_values: Vec<f32>,
    // All channels advance together because CPAL supplies interleaved whole
    // frames. A single counter avoids a modulo and per-channel bookkeeping in
    // the underrun hot path.
    underrun_fade_frames_remaining: usize,
    flush_fade_frames_remaining: usize,
}

impl CallbackState {
    pub(super) fn new(
        data_rx: Consumer<OutputBlock>,
        recycle_tx: Producer<Vec<f32>>,
        callback_alive: Arc<AtomicBool>,
        generation: u64,
        flush_epoch: u64,
        output_channels: usize,
        pending_recycle_capacity: usize,
    ) -> Self {
        let output_channels = output_channels.max(1);
        Self {
            data_rx,
            recycle_tx,
            callback_alive,
            pending_recycles: Vec::with_capacity(pending_recycle_capacity),
            current_block: Vec::new(),
            current_index: 0,
            accepted_generation: generation,
            accepted_flush_epoch: flush_epoch,
            output_channels,
            last_values: vec![0.0; output_channels],
            underrun_fade_frames_remaining: 0,
            flush_fade_frames_remaining: 0,
        }
    }
}

impl Drop for CallbackState {
    fn drop(&mut self) {
        self.callback_alive.store(false, Ordering::Release);
    }
}

pub(super) fn fill_output<T>(
    data: &mut [T],
    state: &mut CallbackState,
    paused: &AtomicBool,
    volume_bits: &AtomicU32,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
    queued_samples: &AtomicUsize,
    rendered_samples: &AtomicU64,
) where
    T: Sample + FromSample<f32>,
{
    sync_output_barrier(state, generation, flush_epoch);

    if paused.load(Ordering::Acquire) {
        data.fill(T::from_sample(0.0));
        state.last_values.fill(0.0);
        state.underrun_fade_frames_remaining = 0;
        state.flush_fade_frames_remaining = 0;
        return;
    }

    let volume = f32::from_bits(volume_bits.load(Ordering::Acquire));
    let channels = state.output_channels;
    let total = data.len();
    let mut pos = fill_flush_fade(data, state, channels);
    let mut queued_written = 0usize;

    // Bulk-copy frame-aligned runs straight from the queued block, applying
    // volume + clamp over a contiguous slice. This lets the conversion loop
    // autovectorize (and collapses to a tight copy on the common f32 device
    // path) instead of branching, taking a modulo, and touching the fade
    // bookkeeping on every single sample. Both cpal buffers and producer
    // blocks are whole frames, so every run is a multiple of `channels` and
    // frame alignment is preserved across block boundaries.
    while pos < total {
        if sync_output_barrier(state, generation, flush_epoch) {
            pos += fill_flush_fade(&mut data[pos..], state, channels);
            if pos >= total {
                break;
            }
        }

        if state.current_index >= state.current_block.len() {
            match next_current_block(state, generation, flush_epoch) {
                Some(block) => {
                    // Return the spent block to the mixer's pool instead of
                    // dropping (freeing) it inside the real-time callback.
                    let spent = std::mem::replace(&mut state.current_block, block);
                    retain_or_recycle_buffer(state, spent);
                    state.current_index = 0;
                }
                None => break,
            }
        }

        let start = state.current_index;
        let avail = state.current_block.len() - start;
        let run = avail.min(total - pos);
        let src = &state.current_block[start..start + run];
        let dst = &mut data[pos..pos + run];
        if volume == 1.0 {
            for (slot, &raw) in dst.iter_mut().zip(src) {
                *slot = T::from_sample(raw.clamp(-1.0, 1.0));
            }
        } else {
            for (slot, &raw) in dst.iter_mut().zip(src) {
                *slot = T::from_sample((raw * volume).clamp(-1.0, 1.0));
            }
        }
        state.current_index += run;
        pos += run;
        queued_written += run;

        // Remember the most recent frame so an underrun can fade from it.
        // `run` is frame-aligned, so the final `channels` samples are exactly
        // one frame in channel order.
        let frame_start = state.current_index - channels;
        for ch in 0..channels {
            let raw = state.current_block[frame_start + ch];
            state.last_values[ch] = (raw * volume).clamp(-1.0, 1.0);
        }
    }

    let written = queued_written;
    if written > 0 {
        // Every channel just received audio, so arm the full anti-click fade
        // window once per callback instead of once per sample.
        state.underrun_fade_frames_remaining = UNDERRUN_FADE_FRAMES;
    }

    // Any tail the queue could not fill decays from the last frame to avoid a click.
    if pos < total {
        fill_underrun_tail(&mut data[pos..], state, channels);
    }

    if written > 0 {
        super::saturating_sub(queued_samples, written);
        // Position accounting is monotonic telemetry; it does not publish
        // audio data or callback state to another thread.
        rendered_samples.fetch_add(written as u64, Ordering::Relaxed);
    }
}

fn next_current_block(
    state: &mut CallbackState,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
) -> Option<Vec<f32>> {
    flush_pending_recycles(state);
    // Replacing the current block may need to retain its allocation. Leave
    // one preallocated slot available before taking ownership of another PCM
    // block, but do not stop merely because older recycle sends are pending.
    if !has_pending_recycle_capacity(state) {
        return None;
    }

    loop {
        match state.data_rx.pop() {
            Ok(block) => {
                let current_generation = generation.load(Ordering::Acquire);
                let current_flush_epoch = flush_epoch.load(Ordering::Acquire);
                if block.generation != current_generation
                    || block.flush_epoch != current_flush_epoch
                {
                    // Stale block from a superseded generation — recycle its
                    // buffer instead of freeing it in the callback.
                    retain_or_recycle_buffer(state, block.samples);
                    if !has_pending_recycle_capacity(state) {
                        return None;
                    }
                    continue;
                }
                if block.samples.is_empty() {
                    retain_or_recycle_buffer(state, block.samples);
                    if !has_pending_recycle_capacity(state) {
                        return None;
                    }
                    continue;
                }
                state.accepted_generation = block.generation;
                state.accepted_flush_epoch = block.flush_epoch;
                return Some(block.samples);
            }
            Err(_) => return None,
        }
    }
}

fn sync_output_barrier(
    state: &mut CallbackState,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
) -> bool {
    let current_generation = generation.load(Ordering::Acquire);
    let current_flush_epoch = flush_epoch.load(Ordering::Acquire);
    if current_generation == state.accepted_generation
        && current_flush_epoch == state.accepted_flush_epoch
    {
        return false;
    }

    apply_output_clear(state, current_generation, current_flush_epoch);
    true
}

fn apply_output_clear(state: &mut CallbackState, generation: u64, flush_epoch: u64) {
    state.accepted_generation = generation;
    state.accepted_flush_epoch = flush_epoch;
    state.current_block.clear();
    state.current_index = 0;
    state.underrun_fade_frames_remaining = 0;
    state.flush_fade_frames_remaining = SEEK_FLUSH_FADE_FRAMES;
}

#[inline]
fn fill_flush_fade<T>(data: &mut [T], state: &mut CallbackState, channels: usize) -> usize
where
    T: Sample + FromSample<f32>,
{
    if state.flush_fade_frames_remaining == 0 {
        return 0;
    }

    let channels = channels.max(1);
    let frames = (data.len() / channels).min(state.flush_fade_frames_remaining);
    let mut pos = 0usize;

    for _ in 0..frames {
        let scale = state.flush_fade_frames_remaining as f32 / SEEK_FLUSH_FADE_FRAMES as f32;
        for ch in 0..channels {
            data[pos + ch] = T::from_sample((state.last_values[ch] * scale).clamp(-1.0, 1.0));
        }
        pos += channels;
        state.flush_fade_frames_remaining -= 1;
    }

    if state.flush_fade_frames_remaining == 0 {
        state.last_values.fill(0.0);
    }

    pos
}

#[inline]
fn fill_underrun_tail<T>(data: &mut [T], state: &mut CallbackState, channels: usize)
where
    T: Sample + FromSample<f32>,
{
    let mut chunks = data.chunks_exact_mut(channels);
    for frame in &mut chunks {
        let remaining = state.underrun_fade_frames_remaining;
        if remaining == 0 {
            frame.fill(T::from_sample(0.0));
            continue;
        }
        let scale = remaining as f32 / UNDERRUN_FADE_FRAMES as f32;
        for (sample, &last) in frame.iter_mut().zip(&state.last_values) {
            *sample = T::from_sample(last * scale);
        }
        state.underrun_fade_frames_remaining = remaining - 1;
    }

    // CPAL buffers are normally frame-aligned. Handle a defensive partial
    // frame without division/modulo in the common path.
    let remaining = state.underrun_fade_frames_remaining;
    let scale = remaining as f32 / UNDERRUN_FADE_FRAMES as f32;
    for (sample, &last) in chunks.into_remainder().iter_mut().zip(&state.last_values) {
        *sample = T::from_sample(last * scale);
    }
    if remaining == 0 {
        state.last_values.fill(0.0);
    }
}

#[inline]
fn retain_or_recycle_buffer(state: &mut CallbackState, buf: Vec<f32>) {
    match state.recycle_tx.push(buf) {
        Ok(()) => {}
        Err(PushError::Full(buf)) => {
            debug_assert!(has_pending_recycle_capacity(state));
            state.pending_recycles.push(buf);
        }
    }
}

#[inline]
fn has_pending_recycle_capacity(state: &CallbackState) -> bool {
    state.pending_recycles.len() < state.pending_recycles.capacity()
}

/// Attempts to hand retained allocations back without blocking. `swap_remove`
/// keeps this bounded and allocation-free; recycle buffer ordering is irrelevant.
fn flush_pending_recycles(state: &mut CallbackState) {
    while let Some(buf) = state.pending_recycles.pop() {
        match state.recycle_tx.push(buf) {
            Ok(()) => {}
            Err(PushError::Full(buf)) => {
                state.pending_recycles.push(buf);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(
        data_capacity: usize,
        recycle_capacity: usize,
        generation: u64,
        flush_epoch: u64,
        channels: usize,
        pending_capacity: usize,
    ) -> (
        rtrb::Producer<OutputBlock>,
        rtrb::Consumer<Vec<f32>>,
        Arc<AtomicBool>,
        CallbackState,
    ) {
        let (data_tx, data_rx) = rtrb::RingBuffer::new(data_capacity);
        let (recycle_tx, recycle_rx) = rtrb::RingBuffer::new(recycle_capacity);
        let callback_alive = Arc::new(AtomicBool::new(true));
        let state = CallbackState::new(
            data_rx,
            recycle_tx,
            Arc::clone(&callback_alive),
            generation,
            flush_epoch,
            channels,
            pending_capacity,
        );
        (data_tx, recycle_rx, callback_alive, state)
    }

    #[test]
    fn clear_output_fades_from_last_frame() {
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(0);
        let rendered_samples = AtomicU64::new(0);
        let (_data_tx, _recycle_rx, _alive, mut state) = test_state(1, 4, 0, 0, 2, 4);
        state.last_values = vec![0.75, -0.5];
        let mut data = vec![0.0f32; 8];

        fill_output(
            &mut data,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );

        assert!(data[0] > 0.7);
        assert!(data[1] < -0.45);
        assert!(data[2].abs() < data[0].abs());
        assert!(data[3].abs() < data[1].abs());
        assert_eq!(rendered_samples.load(Ordering::Acquire), 0);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn stale_blocks_after_flush_are_not_rendered() {
        let (mut data_tx, _recycle_rx, _alive, mut state) = test_state(4, 4, 0, 1, 2, 4);
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(4);
        let rendered_samples = AtomicU64::new(0);
        assert!(data_tx
            .push(OutputBlock {
                samples: vec![0.9, 0.9, 0.9, 0.9],
                generation: 0,
                flush_epoch: 0,
            })
            .is_ok());
        assert!(data_tx
            .push(OutputBlock {
                samples: vec![0.25, -0.25, 0.5, -0.5],
                generation: 0,
                flush_epoch: 1,
            })
            .is_ok());
        let mut data = vec![0.0f32; 4];

        fill_output(
            &mut data,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );

        assert_eq!(data, vec![0.25, -0.25, 0.5, -0.5]);
        assert_eq!(rendered_samples.load(Ordering::Acquire), 4);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn full_recycle_queue_does_not_immediately_stall_pcm_consumption() {
        const PENDING_CAPACITY: usize = 4;
        let (mut data_tx, data_rx) = rtrb::RingBuffer::new(5);
        let (mut recycle_tx, mut recycle_rx) = rtrb::RingBuffer::new(1);
        recycle_tx.push(Vec::with_capacity(1)).unwrap();
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(0);
        let queued_samples = AtomicUsize::new(20);
        let rendered_samples = AtomicU64::new(0);
        let callback_alive = Arc::new(AtomicBool::new(true));
        let mut state = CallbackState::new(
            data_rx,
            recycle_tx,
            Arc::clone(&callback_alive),
            0,
            0,
            2,
            PENDING_CAPACITY,
        );
        state.current_block = Vec::with_capacity(32);
        state
            .current_block
            .extend_from_slice(&[0.1, -0.1, 0.2, -0.2]);
        state.current_index = state.current_block.len();

        for value in [0.3, 0.4, 0.5, 0.6, 0.7] {
            assert!(data_tx
                .push(OutputBlock {
                    samples: vec![value, -value, value, -value],
                    generation: 0,
                    flush_epoch: 0,
                })
                .is_ok());
        }
        let mut data = [0.0f32; 16];

        fill_output(
            &mut data,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );

        assert_eq!(
            data,
            [
                0.3, -0.3, 0.3, -0.3, 0.4, -0.4, 0.4, -0.4, 0.5, -0.5, 0.5, -0.5, 0.6, -0.6, 0.6,
                -0.6,
            ]
        );
        assert_eq!(state.pending_recycles.len(), PENDING_CAPACITY);
        assert_eq!(state.pending_recycles.capacity(), PENDING_CAPACITY);
        assert_eq!(rendered_samples.load(Ordering::Acquire), 16);

        // Once every preallocated overflow slot is occupied, the callback
        // keeps the final queued block untouched rather than freeing a buffer
        // on the real-time thread.
        let mut blocked = [0.0f32; 4];
        fill_output(
            &mut blocked,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );
        assert_eq!(rendered_samples.load(Ordering::Acquire), 16);
        let queued = state.data_rx.pop().unwrap();
        assert_eq!(queued.samples, vec![0.7, -0.7, 0.7, -0.7]);

        // Return that block to the queue, free one recycle slot, and verify
        // normal PCM consumption resumes on the next callback.
        assert!(data_tx.push(queued).is_ok());
        drop(recycle_rx.pop().unwrap());
        fill_output(
            &mut blocked,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );
        assert_eq!(blocked, [0.7, -0.7, 0.7, -0.7]);
        assert_eq!(rendered_samples.load(Ordering::Acquire), 20);
        assert_eq!(state.pending_recycles.len(), PENDING_CAPACITY);
    }

    #[test]
    fn dropping_callback_state_marks_consumer_dead() {
        let (_data_tx, _recycle_rx, callback_alive, state) = test_state(1, 1, 0, 0, 2, 1);
        assert!(callback_alive.load(Ordering::Acquire));
        drop(state);
        assert!(!callback_alive.load(Ordering::Acquire));
    }
}
