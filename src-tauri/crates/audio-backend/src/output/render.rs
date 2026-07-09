use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;

use cpal::{FromSample, Sample};

const UNDERRUN_FADE_FRAMES: usize = 64;
const SEEK_FLUSH_FADE_FRAMES: usize = 256;

pub(super) struct OutputBlock {
    pub(super) samples: Vec<f32>,
    pub(super) generation: u64,
    pub(super) flush_epoch: u64,
}

pub(super) enum OutputControl {
    Clear { generation: u64, flush_epoch: u64 },
}

pub(super) struct CallbackState {
    data_rx: mpsc::Receiver<OutputBlock>,
    control_rx: mpsc::Receiver<OutputControl>,
    /// Returns spent mix blocks to the mixer for reuse instead of freeing them
    /// inside the real-time callback. Best-effort: a full channel just drops
    /// the buffer, so this never blocks.
    recycle_tx: mpsc::SyncSender<Vec<f32>>,
    current_block: Vec<f32>,
    current_index: usize,
    accepted_generation: u64,
    accepted_flush_epoch: u64,
    output_channels: usize,
    last_values: Vec<f32>,
    underrun_fade_remaining: Vec<usize>,
    flush_fade_frames_remaining: usize,
}

impl CallbackState {
    pub(super) fn new(
        data_rx: mpsc::Receiver<OutputBlock>,
        control_rx: mpsc::Receiver<OutputControl>,
        recycle_tx: mpsc::SyncSender<Vec<f32>>,
        generation: u64,
        flush_epoch: u64,
        output_channels: usize,
    ) -> Self {
        let output_channels = output_channels.max(1);
        Self {
            data_rx,
            control_rx,
            recycle_tx,
            current_block: Vec::new(),
            current_index: 0,
            accepted_generation: generation,
            accepted_flush_epoch: flush_epoch,
            output_channels,
            last_values: vec![0.0; output_channels],
            underrun_fade_remaining: vec![0; output_channels],
            flush_fade_frames_remaining: 0,
        }
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
    while let Ok(control) = state.control_rx.try_recv() {
        match control {
            OutputControl::Clear {
                generation,
                flush_epoch,
            } => apply_output_clear(state, generation, flush_epoch),
        }
    }
    sync_output_barrier(state, generation, flush_epoch);

    if paused.load(Ordering::Acquire) {
        data.fill(T::from_sample(0.0));
        state.last_values.fill(0.0);
        state.underrun_fade_remaining.fill(0);
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
                    recycle_buffer(&state.recycle_tx, spent);
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
        state.underrun_fade_remaining.fill(UNDERRUN_FADE_FRAMES);
    }

    // Any tail the queue could not fill decays from the last frame to avoid a click.
    if pos < total {
        for sample_index in pos..total {
            let channel = sample_index % channels;
            let value = underrun_fade_sample(state, channel);
            data[sample_index] = T::from_sample(value);
        }
    }

    if written > 0 {
        super::saturating_sub(queued_samples, written);
        rendered_samples.fetch_add(written as u64, Ordering::AcqRel);
    }
}

fn next_current_block(
    state: &mut CallbackState,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
) -> Option<Vec<f32>> {
    loop {
        match state.data_rx.try_recv() {
            Ok(block) => {
                let current_generation = generation.load(Ordering::Acquire);
                let current_flush_epoch = flush_epoch.load(Ordering::Acquire);
                if block.generation != current_generation
                    || block.flush_epoch != current_flush_epoch
                {
                    // Stale block from a superseded generation — recycle its
                    // buffer instead of freeing it in the callback.
                    recycle_buffer(&state.recycle_tx, block.samples);
                    continue;
                }
                if block.samples.is_empty() {
                    recycle_buffer(&state.recycle_tx, block.samples);
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
    state.underrun_fade_remaining.fill(0);
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
fn underrun_fade_sample(state: &mut CallbackState, channel: usize) -> f32 {
    let remaining = state.underrun_fade_remaining[channel];
    if remaining == 0 {
        return 0.0;
    }

    let scale = remaining as f32 / UNDERRUN_FADE_FRAMES as f32;
    let value = state.last_values[channel] * scale;
    let next_remaining = remaining - 1;
    state.underrun_fade_remaining[channel] = next_remaining;
    if next_remaining == 0 {
        state.last_values[channel] = 0.0;
    }
    value
}

#[inline]
fn recycle_buffer(recycle_tx: &mpsc::SyncSender<Vec<f32>>, buf: Vec<f32>) {
    // Best-effort hand-back to the mixer's pool; a full channel just drops the
    // buffer (a normal free), so the real-time callback never blocks.
    let _ = recycle_tx.try_send(buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_output_fades_from_last_frame() {
        let (_data_tx, data_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(0);
        let rendered_samples = AtomicU64::new(0);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let mut state = CallbackState::new(data_rx, control_rx, recycle_tx, 0, 0, 2);
        state.last_values = vec![0.75, -0.5];
        let mut data = vec![0.0f32; 8];

        control_tx
            .send(OutputControl::Clear {
                generation: 0,
                flush_epoch: 1,
            })
            .unwrap();
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
        let (data_tx, data_rx) = mpsc::sync_channel(4);
        let (_control_tx, control_rx) = mpsc::channel();
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(4);
        let rendered_samples = AtomicU64::new(0);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let mut state = CallbackState::new(data_rx, control_rx, recycle_tx, 0, 1, 2);
        data_tx
            .send(OutputBlock {
                samples: vec![0.9, 0.9, 0.9, 0.9],
                generation: 0,
                flush_epoch: 0,
            })
            .unwrap();
        data_tx
            .send(OutputBlock {
                samples: vec![0.25, -0.25, 0.5, -0.5],
                generation: 0,
                flush_epoch: 1,
            })
            .unwrap();
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
}
