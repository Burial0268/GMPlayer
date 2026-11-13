/**
 * The active sound, as one reactive reference.
 *
 * `window.$player` used to be a bare global: components mirrored it into refs of
 * their own and re-synced those by hand, and call sites reached it three ways
 * (`window.$player`, an untyped `declare const $player`, a cast). It is now an
 * accessor over this ref, so existing reads and writes keep working and anything
 * that reads `activeSound` re-renders when the sound is replaced.
 *
 * New code reads `activeSound` or calls the `*ActiveSound` commands in
 * `PlayerFunctions`; it does not touch `window.$player`.
 */

import { shallowRef } from "vue";
import type { ISound } from "./types";

export const activeSound = shallowRef<ISound | undefined>();

if (typeof window !== "undefined") {
  // Carry over a sound assigned before this module loaded.
  activeSound.value = window.$player;
  Object.defineProperty(window, "$player", {
    configurable: true,
    get: () => activeSound.value,
    set: (sound: ISound | undefined | null) => {
      activeSound.value = sound ?? undefined;
    },
  });
}
