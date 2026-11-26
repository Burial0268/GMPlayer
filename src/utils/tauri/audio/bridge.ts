/**
 * Tauri bridge for the native audio-backend.
 *
 * AMLL-style message/event architecture: a single `audio_send_msg` Tauri
 * command carries every playback command, and events arrive as
 * `AudioThreadEvent`s over the transport's `Channel`.
 *
 * Wire types live in `./protocol`; the timeline reconciler lives in
 * `./timeline`. This module is only the command/event surface.
 */
import { invoke, isTauri } from "../core/runtime";
import type { CommandEnvelope } from "./protocol";

/**
 * Serializes sends so the player thread observes commands in number order —
 * the order every event's `ack` is measured against. Two `invoke`s issued in
 * the same tick can otherwise land out of order.
 */
let audioSendChain: Promise<unknown> = Promise.resolve();

/**
 * Hand one numbered command to the player. Only the transport calls this: it
 * is what numbers commands (see `transport/tauri.ts`). Rejects if this command
 * could not be delivered; later commands are still sent.
 */
export function sendAudioCommand(envelope: CommandEnvelope): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  const sent = audioSendChain.then(() => invoke<void>("audio_send_msg", { msg: envelope }));
  audioSendChain = sent.catch(() => undefined);
  return sent;
}

export async function audioPreheat(): Promise<void> {
  await invoke("audio_preheat");
}

/** Output device names for the player's output menu; empty outside Tauri or on failure. */
export async function audioListOutputDevices(): Promise<string[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<string[]>("audio_list_output_devices");
  } catch (err) {
    console.warn("[audioBridge] audio_list_output_devices failed", err);
    return [];
  }
}
