use super::*;
use crate::types::{
    AudioThreadEventMessage, AudioThreadMessage, AutoMixConfig, NativeSessionSnapshot,
};
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

fn pulse_file(path: &Path, period: f64) {
    let rate = 48_000u32;
    let frames = rate as usize * 16;
    let mut file = std::fs::File::create(path).unwrap();
    let bytes = (frames * 2) as u32;
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x01\0").unwrap();
    file.write_all(&rate.to_le_bytes()).unwrap();
    file.write_all(&(rate * 2).to_le_bytes()).unwrap();
    file.write_all(&2u16.to_le_bytes()).unwrap();
    file.write_all(&16u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&bytes.to_le_bytes()).unwrap();
    let mut pcm = Vec::with_capacity(bytes as usize);
    for frame in 0..frames {
        let time = frame as f64 / rate as f64;
        let phase = (time - 0.17).rem_euclid(period);
        let sample = if time >= 0.17 && phase < 0.025 {
            (std::f64::consts::TAU * 1500.0 * time).sin() * (-phase * 120.0).exp() * 0.5
        } else {
            0.0
        };
        pcm.extend_from_slice(&((sample * i16::MAX as f64) as i16).to_le_bytes());
    }
    file.write_all(&pcm).unwrap();
}

#[test]
#[ignore = "isolated PulseAudio: scripts/test-audio-device-watch.sh live_native_automix"]
fn live_native_automix_follows_rendered_audio_and_pauses_without_a_timer_handoff() {
    assert_eq!(
        std::env::var("GMPLAYER_DEVICE_WATCH_TEST").as_deref(),
        Ok("1")
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a.wav");
        let b = directory.path().join("b.wav");
        pulse_file(&a, 0.5);
        pulse_file(&b, 0.5 / 1.03);
        let song = |path: &Path, index| SongData::Local {
            file_path: path.to_string_lossy().to_string(), orig_order: index, display: None,
        };
        let a = song(&a, 0);
        let b = song(&b, 1);
        let (_, messages) = tokio::sync::mpsc::unbounded_channel();
        let (_, seeks) = tokio::sync::mpsc::unbounded_channel();
        let (events, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut player = AudioPlayer::new(messages, seeks, events,
            Arc::new(parking_lot::Mutex::new(NativeSessionSnapshot::default()))).await.unwrap();
        player.playback_intent = PlaybackIntent::Playing;
        player.analysis_enabled.store(false, Ordering::Release);
        let message = |data| AudioThreadEventMessage::new("live-test".into(), Some(data));
        player.process_message(message(AudioThreadMessage::SetPlaylist {
            songs: vec![a.clone(), b.clone()], windowed: true, play_index: Some(0), initial_position: Some(8.0), load_request_id: None,
        })).await.unwrap();
        player.process_message(message(AudioThreadMessage::AutomixConfigure {
            config: AutoMixConfig {
                enabled: true, crossfade_duration: 4.0, smart_curve: false,
                emotional_transition: false, volume_norm: false, transition_effects: false,
                vocal_guard: false, ..AutoMixConfig::default()
            },
        })).await.unwrap();
        player.process_message(message(AudioThreadMessage::AutomixPrepareNext {
            current_index: 0, next_index: 1, next_song: b.clone(), transition_id: Some(77),
            current_identity: Some(crate::types::TrackIdentity::Local { path: a.file_path().unwrap().to_string() }),
            next_identity: Some(crate::types::TrackIdentity::Netease { id: "2".into() }),
        })).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(8), player.automix_prepare_rx.recv()).await.unwrap().unwrap();
        player.handle_automix_prepare_result(result).await;
        assert!(player.native_mix.is_some(), "native plan was not armed");
        let deadline = Instant::now() + Duration::from_secs(45);
        eprintln!("armed status: {:?}, position={}", player.automix.status(0), player.clock_position());
        let mut started = false;
        let mut track_epochs: Vec<u64> = Vec::new();
        let mut last_change_requested = true;
        let mut completed = false;
        let mut paused = false;
        let mut rebuilt = false;
        let mut fade_duration = 0.0;
        while Instant::now() < deadline {
            if !rebuilt && player.output.writer().render_clock().anchor().is_some_and(|anchor| anchor.anchor.phase == 2) {
                // A route rebuild can land after B rendered but before the player adopted it.
                let output = tokio::task::spawn_blocking(|| crate::output::open_output(crate::output::OutputDeviceSelector::Default, None)).await.unwrap().unwrap();
                player.handle_output_refresh_event(crate::player::output_runtime::OutputRefreshEvent::Opened {
                    generation: player.output_refresh_generation, output_epoch: player.output_epoch,
                    output, force_replace: true, rebuild_chain: true,
                }).await;
                let ready = tokio::time::timeout(Duration::from_secs(8), player.output_refresh_rx.recv()).await.unwrap().unwrap();
                player.handle_output_refresh_event(ready).await;
                assert_eq!(player.current_song.as_ref().unwrap().get_id(), b.get_id());
                assert!(player.clock_position() < 8.0, "rebuild applied A's old position to B");
                rebuilt = true;
            }
            player.tick_native_mix().await;
            while let Ok(event) = player.decoder_event_rx.try_recv() {
                match event {
                    decoder::DecoderEvent::Finished { playback_id } => player.handle_decoder_finished(playback_id).await,
                    decoder::DecoderEvent::Starved { .. } => panic!("local audio starved"),
                }
            }
            while let Ok(event) = event_rx.try_recv() {
                match event.data {
                    Some(AudioThreadEvent::AutomixCrossfadeStarted { duration, .. }) => {
                        started = true;
                        fade_duration = duration;
                        let anchor = player.output.writer().render_clock().anchor().unwrap();
                        assert!((1.01..=1.04).contains(&anchor.anchor.source.rate), "tempo plan did not engage: {anchor:?}");
                    }
                    Some(AudioThreadEvent::TrackChanged { timeline_epoch, load_request_id, .. }) => {
                        assert!(!track_epochs.contains(&timeline_epoch), "one edge per timeline");
                        track_epochs.push(timeline_epoch);
                        last_change_requested = load_request_id.is_some();
                    }
                    Some(AudioThreadEvent::AutomixCrossfadeComplete { position, transition_id, .. }) => {
                        assert!(track_epochs.len() >= 2 && !last_change_requested, "the hand-off's TrackChanged precedes its completion");
                        assert_eq!(transition_id, Some(78));
                        assert!(position >= fade_duration - 0.01 && position < fade_duration + 1.2, "B position={position}, fade={fade_duration}");
                        completed = true;
                    }
                    Some(AudioThreadEvent::AutomixError { error, .. }) => panic!("AutoMix failed: {error}"),
                    _ => {}
                }
            }
            if started && !paused {
                player.process_message(message(AudioThreadMessage::PauseAudio)).await.unwrap();
                let position = player.clock_position();
                tokio::time::sleep(Duration::from_millis(250)).await;
                player.tick_native_mix().await;
                assert_eq!(player.clock_position(), position);
                assert_eq!(player.current_song.as_ref().unwrap().get_id(), a.get_id());
                assert!(player.native_crossfade_active);
                player.process_message(message(AudioThreadMessage::ResumeAudio)).await.unwrap();
                let generation = player.native_crossfade_gen();
                let incoming = player.secondary_playback_id;
                player.process_message(message(AudioThreadMessage::AutomixPrepareNext {
                    current_index: 0, next_index: 7,
                    next_song: SongData::Local { file_path: "https://example.invalid/rotated-signed-url".into(), orig_order: 7, display: None },
                    transition_id: Some(78),
                    current_identity: Some(crate::types::TrackIdentity::Local { path: a.file_path().unwrap().to_string() }),
                    next_identity: Some(crate::types::TrackIdentity::Netease { id: "2".into() }),
                })).await.unwrap();
                assert_eq!(player.native_crossfade_gen(), generation, "a recreated page must attach to the same transition");
                assert_eq!(player.secondary_playback_id, incoming);
                paused = true;
            }
            if completed && rebuilt { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(started && paused && completed, "transition did not complete: started={started}, paused={paused}, completed={completed}");
        assert!(rebuilt, "output rebuild at handoff was not exercised");
        assert_eq!(player.current_song.as_ref().unwrap().get_id(), b.get_id());
        assert_eq!(player.current_play_index, 7);
        assert_eq!(player.current_song.as_ref().unwrap().orig_order(), 7);
        assert_eq!(player.current_file_path.as_deref(), b.file_path(), "handoff must adopt B's source locator as well as its decoder");
        assert!((player.clock_position() - fade_duration).abs() < 1.2);
        let generation = player.native_crossfade_gen();
        player.process_message(message(AudioThreadMessage::AutomixPrepareNext {
            current_index: 0, next_index: 1, next_song: b.clone(), transition_id: Some(79),
            current_identity: Some(crate::types::TrackIdentity::Local { path: a.file_path().unwrap().to_string() }),
            next_identity: Some(crate::types::TrackIdentity::Netease { id: "2".into() }),
        })).await.unwrap();
        assert_eq!(player.native_crossfade_gen(), generation, "a stale page must not prepare A after B was adopted");
        assert_eq!(player.current_song.as_ref().unwrap().get_id(), b.get_id());
        player.process_message(message(AudioThreadMessage::JumpToSong { song_index: 0 })).await.unwrap();
        assert_eq!(player.current_local_path.as_deref(), Some(Path::new(a.file_path().unwrap())), "previous must reopen A rather than reusing B's decoded source");
    });
}
