import { shallowRef } from "vue";
import type { AMLLLine } from "@/utils/LyricsProcessor";
import { useLayerNavigation } from "@/utils/navigation";

/** Where the pressed line sat and how AMLL drew it, so the share list can take its place. */
export interface LyricShareOrigin {
  rect: DOMRect;
  fontSize: number;
  padding: string;
}

export interface LyricShareRequest {
  lines: AMLLLine[];
  lineIndex: number;
  origin: LyricShareOrigin;
}

// The lyrics view asks, the player's share layer answers: they sit in different
// branches of the player and the layer history cannot carry DOM geometry.
const request = shallowRef<LyricShareRequest>();

export function useLyricShare() {
  const navigation = useLayerNavigation();
  return {
    request,
    open(next: LyricShareRequest) {
      request.value = next;
      void navigation.openShare();
    },
    /** Called once the layer has left, so the content stays put while it fades. */
    clear() {
      request.value = undefined;
    },
  };
}
