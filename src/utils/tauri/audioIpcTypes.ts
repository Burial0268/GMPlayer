import type { AudioThreadEventCallback, AudioThreadMessage } from "./audioBridge";

export interface AudioBackendTransport {
  connect(): Promise<void>;
  subscribe(listener: AudioThreadEventCallback): () => void;
  sendOrQueue(msg: AudioThreadMessage): boolean;
  getGainNode?(): GainNode | null;
  ensureAudioGraph?(): Promise<boolean>;
  shutdown?(): void;
}
