//! Local WebSocket bridge for low-latency audio backend communication.
//!
//! The Tauri IPC channel has unbounded queueing on Windows that adds 10–40 ms
//! per round-trip and back-pressures when the webview is unfocused — which
//! caused user-visible stutter in spectrum / position updates. A 127.0.0.1
//! WebSocket is sub-millisecond and decoupled from the webview's event loop.
//!
//! Architecture:
//! ```text
//!  Audio thread (player::AudioPlayer)
//!     │  evt_sender (mpsc) ── push AudioThreadEventMessage<AudioThreadEvent>
//!     ▼
//!  forwarder task (in Player::new) ─► broadcast::Sender<String>
//!                                          │
//!                                          ▼
//!                                   per-connection send loop
//!                                          │
//!                                          ▼
//!                                       WebSocket
//!                                          │
//!                                          ▼
//!                                     JS audioWs.ts (event socket)
//!
//!  JS audioWs.ts (control socket) ──► command read loop ──► PlayerHandle.send(msg)
//!                                ▲
//!                                └── priority status/control events
//! ```
//!
//! Control and event traffic intentionally use separate WebSocket
//! connections. High-rate FFT/event delivery can be dropped/coalesced, but
//! playback controls and their status confirmations must not wait behind
//! event serialization, browser event parsing, or a slow outbound sink.

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::player::PlayerHandle;
use crate::types::{AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage};

/// How many in-flight broadcast messages we buffer per subscriber before
/// the slowest one is forcibly disconnected. FFT events at 20 Hz +
/// position events at 30 Hz means a small backlog can happen during GC
/// pauses — 256 is enough headroom for ~5 seconds.
const BROADCAST_CAPACITY: usize = 256;
/// Priority status/control events are duplicated onto the control socket so
/// play/pause/seek confirmations never sit behind FFT frames on the event
/// socket. This channel is low-rate, so a small backlog is enough.
const PRIORITY_BROADCAST_CAPACITY: usize = 64;

const CTRL_RESUME: u8 = 1;
const CTRL_PAUSE: u8 = 2;
const CTRL_TOGGLE: u8 = 3;
const CTRL_SEEK: u8 = 4;
const CTRL_SET_VOLUME: u8 = 5;
const CTRL_SET_VOLUME_RELATIVE: u8 = 6;

pub struct WsServer {
    /// Event socket address (Rust → frontend).
    event_addr: SocketAddr,
    /// Control socket address (frontend → Rust).
    control_addr: SocketAddr,
    /// Outbound event broadcast — every connection gets its own subscriber.
    event_tx: broadcast::Sender<String>,
    /// Low-rate playback/status events duplicated to control connections.
    priority_event_tx: broadcast::Sender<String>,
    /// Keep listener tasks alive for the lifetime of the server.
    _event_listener_task: tokio::task::JoinHandle<()>,
    _control_listener_task: tokio::task::JoinHandle<()>,
}

impl WsServer {
    /// Bind two `127.0.0.1:0` listeners (kernel picks ports). Returns once
    /// both listeners are accepting connections.
    pub async fn start(player_handle: PlayerHandle) -> anyhow::Result<Self> {
        let event_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let control_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let event_addr = event_listener.local_addr()?;
        let control_addr = control_listener.local_addr()?;
        info!("音频事件 WebSocket 已开启: ws://{event_addr}");
        info!("音频控制 WebSocket 已开启: ws://{control_addr}");

        let (event_tx, _) = broadcast::channel::<String>(BROADCAST_CAPACITY);
        let event_tx_for_task = event_tx.clone();
        let (priority_event_tx, _) = broadcast::channel::<String>(PRIORITY_BROADCAST_CAPACITY);
        let priority_event_tx_for_task = priority_event_tx.clone();

        let event_listener_task = tokio::spawn(async move {
            loop {
                match event_listener.accept().await {
                    Ok((stream, peer)) => {
                        debug!("接受事件 WebSocket 连接: {peer}");
                        let event_rx = event_tx_for_task.subscribe();
                        tokio::spawn(handle_event_connection(stream, peer, event_rx));
                    }
                    Err(e) => {
                        warn!("事件 WebSocket accept 出错: {e:?}");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        let control_listener_task = tokio::spawn(async move {
            loop {
                match control_listener.accept().await {
                    Ok((stream, peer)) => {
                        debug!("接受控制 WebSocket 连接: {peer}");
                        let player = player_handle.clone();
                        let priority_event_rx = priority_event_tx_for_task.subscribe();
                        tokio::spawn(handle_control_connection(
                            stream,
                            peer,
                            player,
                            priority_event_rx,
                        ));
                    }
                    Err(e) => {
                        warn!("控制 WebSocket accept 出错: {e:?}");
                        // Brief back-off so we don't tight-loop on a fatal accept error.
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(Self {
            event_addr,
            control_addr,
            event_tx,
            priority_event_tx,
            _event_listener_task: event_listener_task,
            _control_listener_task: control_listener_task,
        })
    }

    /// Backward-compatible alias for the event socket.
    pub fn ws_url(&self) -> String {
        self.event_ws_url()
    }

    pub fn event_ws_url(&self) -> String {
        format!("ws://{}", self.event_addr)
    }

    pub fn control_ws_url(&self) -> String {
        format!("ws://{}", self.control_addr)
    }

    pub fn port(&self) -> u16 {
        self.event_addr.port()
    }

    /// Serialize the event and broadcast to every connected client. If no
    /// clients are connected the message is dropped before serialization, so
    /// high-rate FFT frames have zero cost while the frontend is disconnected.
    /// The audio thread must never block on the network.
    pub fn broadcast_event(&self, evt: &AudioThreadEventMessage<AudioThreadEvent>) {
        let has_event_subscribers = self.event_tx.receiver_count() > 0;
        let has_priority_subscribers =
            self.priority_event_tx.receiver_count() > 0 && is_priority_event(evt);

        if !has_event_subscribers && !has_priority_subscribers {
            return;
        }

        let json = match serde_json::to_string(evt) {
            Ok(s) => s,
            Err(e) => {
                warn!("WS 事件序列化失败: {e:?}");
                return;
            }
        };
        if has_priority_subscribers {
            // Control/status events are low-rate and latency-sensitive. Duplicate
            // them onto the control socket before the general event broadcast.
            let _ = self.priority_event_tx.send(json.clone());
        }
        if has_event_subscribers {
            // send() errors only when there are zero subscribers. That's fine —
            // it means the frontend disconnected between the receiver_count check
            // and send; events resume once a client reconnects.
            let _ = self.event_tx.send(json);
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.event_tx.receiver_count()
    }
}

async fn handle_event_connection(
    stream: TcpStream,
    peer: SocketAddr,
    mut event_rx: broadcast::Receiver<String>,
) {
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket 握手失败 ({peer}): {e:?}");
            return;
        }
    };
    info!("事件 WebSocket 客户端已连接: {peer}");

    let (mut sink, mut stream) = ws.split();

    // Outbound: pump broadcast → sink.
    let outbound = async move {
        loop {
            match event_rx.recv().await {
                Ok(json) => {
                    if let Err(e) = sink.send(Message::Text(json)).await {
                        debug!("WS 发送失败 ({peer}): {e:?}");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WS 客户端 {peer} 落后 {n} 条事件，已丢弃");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        let _ = sink.close().await;
    };

    // Inbound on the event socket only keeps the connection lifecycle honest.
    // Commands never travel here, so event backpressure cannot delay controls.
    let inbound = async move {
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    debug!("WS 读取失败 ({peer}): {e:?}");
                    break;
                }
            };
            match msg {
                Message::Close(_) => break,
                Message::Text(_)
                | Message::Ping(_)
                | Message::Pong(_)
                | Message::Binary(_)
                | Message::Frame(_) => {}
            }
        }
    };

    // Either direction closing ends the connection — `select!` cancels the
    // other side.
    tokio::select! {
      _ = outbound => {},
      _ = inbound => {},
    }
    info!("事件 WebSocket 客户端已断开: {peer}");
}

async fn handle_control_connection(
    stream: TcpStream,
    peer: SocketAddr,
    player: PlayerHandle,
    mut priority_event_rx: broadcast::Receiver<String>,
) {
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("控制 WebSocket 握手失败 ({peer}): {e:?}");
            return;
        }
    };
    info!("控制 WebSocket 客户端已连接: {peer}");

    let (mut sink, mut stream) = ws.split();

    let outbound = async move {
        loop {
            match priority_event_rx.recv().await {
                Ok(json) => {
                    if let Err(e) = sink.send(Message::Text(json)).await {
                        debug!("控制 WS 优先事件发送失败 ({peer}): {e:?}");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("控制 WS 客户端 {peer} 落后 {n} 条优先事件，已丢弃");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        let _ = sink.close().await;
    };

    let inbound = async move {
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    debug!("控制 WS 读取失败 ({peer}): {e:?}");
                    break;
                }
            };
            match msg {
                Message::Text(text) => {
                    if let Err(e) = dispatch_text(&text, &player) {
                        warn!("控制 WS 命令分发失败: {e:?}");
                    }
                }
                Message::Binary(bytes) => {
                    if let Err(e) = dispatch_binary(&bytes, &player) {
                        warn!("控制 WS binary 命令分发失败: {e:?}");
                    }
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    };

    tokio::select! {
      _ = outbound => {},
      _ = inbound => {},
    }

    info!("控制 WebSocket 客户端已断开: {peer}");
}

fn dispatch_text(text: &str, player: &PlayerHandle) -> anyhow::Result<()> {
    let msg: AudioThreadEventMessage<AudioThreadMessage> = serde_json::from_str(text)?;
    player
        .send(msg)
        .map_err(|e| anyhow::anyhow!("player channel closed: {e:?}"))?;
    Ok(())
}

fn dispatch_binary(bytes: &[u8], player: &PlayerHandle) -> anyhow::Result<()> {
    let Some((&opcode, payload)) = bytes.split_first() else {
        anyhow::bail!("empty binary control frame");
    };

    let msg = match opcode {
        CTRL_RESUME => AudioThreadMessage::ResumeAudio,
        CTRL_PAUSE => AudioThreadMessage::PauseAudio,
        CTRL_TOGGLE => AudioThreadMessage::ResumeOrPauseAudio,
        CTRL_SEEK => AudioThreadMessage::SeekAudio {
            position: read_f64_payload(payload)?,
            request_id: None,
            expected_music_id: None,
        },
        CTRL_SET_VOLUME => AudioThreadMessage::SetVolume {
            volume: read_f64_payload(payload)?,
        },
        CTRL_SET_VOLUME_RELATIVE => AudioThreadMessage::SetVolumeRelative {
            volume: read_f64_payload(payload)?,
        },
        _ => anyhow::bail!("unknown binary control opcode: {opcode}"),
    };

    player
        .send(AudioThreadEventMessage::new(String::new(), Some(msg)))
        .map_err(|e| anyhow::anyhow!("player channel closed: {e:?}"))?;
    Ok(())
}

fn read_f64_payload(payload: &[u8]) -> anyhow::Result<f64> {
    if payload.len() != 8 {
        anyhow::bail!("invalid f64 payload length: {}", payload.len());
    }
    let bytes: [u8; 8] = payload
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid f64 payload"))?;
    Ok(f64::from_le_bytes(bytes))
}

fn is_priority_event(evt: &AudioThreadEventMessage<AudioThreadEvent>) -> bool {
    !matches!(
        evt.data,
        Some(AudioThreadEvent::FFTData { .. } | AudioThreadEvent::LowFrequencyVolume { .. })
    )
}

// We don't need a special Drop impl: the listener task is detached and
// will keep running until the process exits (which is what we want — the
// server lives as long as the audio backend).
#[allow(dead_code)]
fn _ws_server_traits_unsync(s: WsServer) -> Arc<WsServer> {
    Arc::new(s)
}
