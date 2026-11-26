/** The lyric font when nothing is chosen. Not bundled: it is used only where it is installed. */
export const DEFAULT_LYRIC_FONT = "HarmonyOS Sans SC";

/**
 * A chosen font family followed by the UI stack.
 *
 * A bare family that is not installed does not fall back to the surrounding UI font: the
 * browser goes to its own default, which on Chinese Windows is a Song or FangSong face.
 */
export const lyricFontStack = (family?: string | null): string =>
  `"${family || DEFAULT_LYRIC_FONT}", var(--font-family-ui)`;
