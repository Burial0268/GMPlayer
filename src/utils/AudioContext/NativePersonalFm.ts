import { isTauri } from "@/utils/tauri/core/runtime";
import { getAudioBackendTransport } from "@/utils/tauri/audio/transport";
import { setNativePersonalFmActive } from "@/utils/tauri/audio/nativeRustSound";
import type { AudioThreadEvent, NativeFmSnapshot } from "@/utils/tauri/audio/protocol";
import useMusicDataStore from "@/store/musicData";
import { syncNativeResolverConfig } from "./NativeResolverConfigSync";

let acceptSession = true;
let stoppedSessionId: number | null = null;

/** The command pipe is shared with normal playback, preserving stop → load order. */
export const startNativePersonalFm = (seed: Record<string, any> | null): void => {
  if (!isTauri()) return;
  acceptSession = true;
  stoppedSessionId = null;
  setNativePersonalFmActive(true);
  syncNativeResolverConfig({ force: true });
  getAudioBackendTransport().sendOrQueue({ type: "startPersonalFm", seed });
};

export const stopNativePersonalFm = (sessionId: number | null): void => {
  if (!isTauri()) return;
  acceptSession = false;
  setNativePersonalFmActive(false);
  if (sessionId === null) {
    // The start reply may not have arrived yet; stop before the next ordinary load.
    getAudioBackendTransport().sendOrQueue({ type: "stopPersonalFm" });
  } else if (stoppedSessionId !== sessionId) {
    stoppedSessionId = sessionId;
    getAudioBackendTransport().sendOrQueue({ type: "stopPersonalFm", sessionId });
  }
};

/** Also used by the boot snapshot, before the first audio controller exists. */
export const adoptNativePersonalFm = (session: NativeFmSnapshot | null): boolean => {
  if (session && !acceptSession) {
    // An enter immediately followed by exit may precede the first session-id reply.
    stopNativePersonalFm(session.sessionId);
    return false;
  }
  const music = useMusicDataStore();
  const accepted = music.adoptNativeFmSession(session);
  if (accepted) setNativePersonalFmActive(session !== null);
  return accepted;
};

export const sendNativeFmPlaying = (playing: boolean): void => {
  getAudioBackendTransport().sendOrQueue({ type: playing ? "resumeAudio" : "pauseAudio" });
};

export const nextNativePersonalFm = (): void => {
  getAudioBackendTransport().sendOrQueue({ type: "nextSong" });
};

export const trashNativePersonalFm = (sessionId: number, id: string): void => {
  getAudioBackendTransport().sendOrQueue({ type: "trashPersonalFm", sessionId, id });
};

/** Subscribe before boot adoption so a renderer reload cannot miss radio state. */
export const setupNativePersonalFm = async (
  onPlayback: (event: Extract<AudioThreadEvent, { type: "loadAudio" | "syncStatus" }>) => void,
): Promise<() => void> => {
  if (!isTauri()) return () => {};
  const transport = getAudioBackendTransport();
  await transport.connect();
  let lastSequence = 0;
  return transport.subscribe((event, sequence) => {
    if (sequence && sequence <= lastSequence) return;
    if (sequence) lastSequence = sequence;
    if (event.type === "personalFmChanged") {
      adoptNativePersonalFm(event.data.session);
    } else if (event.type === "personalFmTrashResult") {
      useMusicDataStore().applyNativeFmTrashResult(event.data);
    } else if (
      (event.type === "loadAudio" || event.type === "syncStatus") &&
      useMusicDataStore().nativeFmSession &&
      acceptSession
    ) {
      onPlayback(event);
    }
  });
};
