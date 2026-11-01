//! A streamed track whose download fell behind its playback.
//!
//! The decoder owns the detection (`decoder::StreamGate`); this is the player's
//! half. Holding the decoder is not enough: the clock counts samples the output
//! renders, so an output left running on an empty ring would play silence and
//! carry the position on. The output is paused and the anchor frozen instead,
//! and the frontend is told so it can show a rebuffer rather than a pause.

use tracing::info;

use crate::types::AudioThreadEvent;

use super::{AudioPlayer, PlaybackIntent};

impl AudioPlayer {
    pub(super) async fn handle_decoder_starved(&mut self, playback_id: u64, starved: bool) {
        // A superseded decoder's last word, or the incoming deck of a crossfade.
        if playback_id != self.decoder_playback_id || self.stream_buffering == starved {
            return;
        }
        self.stream_buffering = starved;
        info!("下载落后于播放: buffering={starved}");

        // Frozen at the audible position, before the anchor is moved.
        let position = self.clock_position();
        if self.playback_intent == PlaybackIntent::Playing {
            if starved {
                self.output.writer().set_paused(true);
                self.publish_position_anchor(false, position).await;
            } else {
                // The flag is already clear, so this really unpauses.
                self.resume_audio_output().await;
                self.publish_position_anchor(true, position).await;
            }
        }

        let music_id = self.session.lock().music_id.clone();
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::Buffering {
                music_id,
                is_buffering: starved,
            })
            .await;
        self.publish_now_playing().await;
    }
}
