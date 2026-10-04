import type { NativeFmSnapshot } from "@/utils/tauri/audio/protocol";

/** The store uses numeric ids; refuse an identity it cannot represent exactly. */
export const fmSongId = (value: unknown): number | null => {
  const id = Number(value);
  return Number.isSafeInteger(id) && id > 0 ? id : null;
};

export const isNewNativeFmSnapshot = (
  previous: NativeFmSnapshot | null,
  incoming: NativeFmSnapshot,
): boolean =>
  !previous ||
  incoming.sessionId > previous.sessionId ||
  (incoming.sessionId === previous.sessionId && incoming.revision > previous.revision);
