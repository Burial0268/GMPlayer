import type { AudioThreadEventCallback, AudioThreadMessage, EventMeta } from "../protocol";

/**
 * What `send` hands back. The backend stamps every event with the last command
 * it had taken up (`EventMeta.ack`), so an event that does not acknowledge a
 * command was emitted before that command took effect and cannot describe the
 * state it asked for. A command the backend never received counts as
 * acknowledged, so nothing waits on it forever.
 */
export interface CommandTicket {
  acknowledgedBy(meta: EventMeta): boolean;
}

export interface AudioBackendTransport {
  /** Resolves once the stream is live: on Tauri, once its first `hello` was delivered. */
  connect(): Promise<void>;
  /** Every event once, in the backend's order, each with the meta it was stamped with. */
  subscribe(listener: AudioThreadEventCallback): () => void;
  send(msg: AudioThreadMessage): CommandTicket;
  /**
   * Ask for a fresh snapshot. It arrives as a `hello` in the stream, in order,
   * before this resolves. Tauri only: the Web backend lives with the page.
   */
  resync?(): Promise<void>;
  getGainNode?(): GainNode | null;
  ensureAudioGraph?(): Promise<boolean>;
  shutdown?(): void;
}
