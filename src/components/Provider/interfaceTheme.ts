import {
  Contrast,
  Hct,
  argbFromHex,
  argbFromRgb,
  hexFromArgb,
  lstarFromArgb,
} from "@material/material-color-utilities";
import type { GlobalThemeOverrides } from "naive-ui";

interface AccentColors {
  primaryColor: string;
  primaryColorHover: string;
  primaryColorPressed: string;
  primaryColorSuppl: string;
}

let colorContext: CanvasRenderingContext2D | null;

const resolveColor = (color: string, background: string) => {
  if (/^#(?:[\da-f]{3}|[\da-f]{6})$/i.test(color)) return argbFromHex(color);
  // The existing color picker also accepts CSS rgba/hsl and alpha-last hex colors.
  if (!colorContext) {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    colorContext = canvas.getContext("2d", { willReadFrequently: true })!;
  }
  colorContext.fillStyle = background;
  colorContext.fillRect(0, 0, 1, 1);
  colorContext.fillStyle = color;
  colorContext.fillRect(0, 0, 1, 1);
  const [r, g, b] = colorContext.getImageData(0, 0, 1, 1).data;
  return argbFromRgb(r, g, b);
};

export const textOnColor = (color = "#f55e55", background = "#ffffff") => {
  const tone = lstarFromArgb(resolveColor(color, background));
  return Contrast.ratioOfTones(tone, 100) >= Contrast.ratioOfTones(tone, 0) ? "#ffffff" : "#000000";
};

// Filled accent controls follow Apple's convention — white on saturated fills, flipping to
// black only once the accent itself is light (bright yellows/greens, or any accent on a dark
// appearance). A tone gate reproduces that across appearances; because every accent is floored
// to ≥3:1 against white, white text on a light-mode accent clears the same bar.
const textOnAccent = (hex: string) => (Hct.fromInt(argbFromHex(hex)).tone >= 63 ? "#000000" : "#ffffff");

// sRGB relative luminance (WCAG) straight off the rendered color — chroma-aware, unlike a
// tone-only estimate, which matters when checking a saturated accent against the surface.
const relativeLuminance = (argb: number) => {
  const channel = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return (
    0.2126 * channel((argb >> 16) & 0xff) +
    0.7152 * channel((argb >> 8) & 0xff) +
    0.0722 * channel(argb & 0xff)
  );
};

// HCT-hue → tone anchors sampled from Apple's systemColors, per appearance. Brown is left out
// on purpose: it is a desaturated orange, so its low tone would drag the curve down through the
// orange/yellow arc, where the system colors actually sit bright.
const ACCENT_TONE_ANCHORS: Record<"light" | "dark", number[][]> = {
  light: [
    [15.4, 56], [26, 57], [62.7, 71], [91.7, 84], [148.8, 71],
    [214.1, 66], [265.8, 53], [286.1, 44], [318.6, 52],
  ],
  dark: [
    [13.9, 57], [25.5, 58], [67.5, 74], [96.2, 87], [148.2, 74],
    [213.3, 75], [262.8, 56], [286, 47], [319, 57],
  ],
};

// Piecewise-linear interpolation of the tone curve around the hue wheel (wraps past the last
// anchor back to the first).
const toneForHue = (hue: number, anchors: number[][]) => {
  const h = ((hue % 360) + 360) % 360;
  for (let i = 0; i < anchors.length; i++) {
    const [h0, t0] = anchors[i];
    const next = anchors[(i + 1) % anchors.length];
    const h1 = i === anchors.length - 1 ? next[0] + 360 : next[0];
    const hh = i === anchors.length - 1 && h < h0 ? h + 360 : h;
    if (hh >= h0 && hh <= h1) return t0 + (next[1] - t0) * ((hh - h0) / (h1 - h0));
  }
  return anchors[0][1];
};

export function createInterfaceTheme(
  source: AccentColors,
  options: { dark: boolean; mobile: boolean; increasedContrast: boolean },
) {
  const { dark, mobile, increasedContrast } = options;
  const surface = dark ? "#18181c" : "#ffffff";
  const primaryText = dark ? "#f5f5f7" : "#1d1d1f";
  const secondaryText = increasedContrast ? primaryText : dark ? "#c7c7cc" : "#57575f";
  const tertiaryText = increasedContrast ? primaryText : dark ? "#ababb2" : "#5f5f67";
  const elevated = dark ? "#26262b" : "#ffffff";
  const separator = dark ? "rgba(255, 255, 255, 0.14)" : "rgba(0, 0, 0, 0.1)";
  const border = increasedContrast ? (dark ? "#98989f" : "#76767e") : separator;
  // Apple keeps its system colors vivid *and* legible by seating each hue at the tone where
  // that hue reads saturated on the current background — not by flattening every hue to one
  // tone, which turns orange/green/yellow to mud. We rebuild that tone-by-hue curve, keep
  // muted brand hues where they are, then push chroma to the gamut edge.
  const anchors = dark ? ACCENT_TONE_ANCHORS.dark : ACCENT_TONE_ANCHORS.light;
  const boost = increasedContrast ? 1 : 1.25;
  // --main-color paints colored text/icons in ~90 places, so guard legibility on the surface:
  // 3:1 is WCAG's bar for UI/large text and is exactly where Apple's vivid system colors sit;
  // Increased Contrast tightens to 4.5:1 (AA body text).
  const contrastFloor = increasedContrast ? 4.5 : 3;
  const surfaceLuminance = relativeLuminance(resolveColor(surface, surface));
  const contrastOnSurface = (argb: number) => {
    const lum = relativeLuminance(argb);
    const hi = Math.max(lum, surfaceLuminance);
    const lo = Math.min(lum, surfaceLuminance);
    return (hi + 0.05) / (lo + 0.05);
  };
  // The accent's four states share one hue/chroma; only tone shifts, so drive them all from the
  // primary swatch (album-art and custom accents only ever supply a primary anyway).
  const primaryHct = Hct.fromInt(resolveColor(source.primaryColor, surface));
  // Asking for more chroma than a tone can hold is safe — HCT maps it back to the most saturated
  // in-gamut color, so already-muted brand hues (navy, brown) stay muted.
  const seatChroma = primaryHct.chroma * boost;
  const seat = (tone: number) =>
    hexFromArgb(Hct.from(primaryHct.hue, seatChroma, Math.max(2, Math.min(96, tone))).toInt());
  // Blend the source tone toward Apple's curve by how saturated the brand hue is: vivid hues snap
  // to the system tone, muted ones (navy #3b5998, brown) keep their own depth.
  const saturationWeight = Math.min(primaryHct.chroma / 48, 1);
  let baseTone =
    primaryHct.tone * (1 - saturationWeight) + toneForHue(primaryHct.hue, anchors) * saturationWeight;
  // Then step toward the background's high-contrast side until the floor is met: darker in light
  // mode, brighter in dark mode (each raises contrast against its own background).
  const step = dark ? 1 : -1;
  for (let i = 0; i < 70 && contrastOnSurface(resolveColor(seat(baseTone), surface)) < contrastFloor; i++) {
    baseTone += step;
    if (baseTone <= 2 || baseTone >= 96) break;
  }
  // Hover/pressed shift tone the way native controls do: dark UIs lift on hover, light UIs deepen.
  const accent = {
    primaryColor: seat(baseTone),
    primaryColorHover: seat(baseTone + (dark ? 4 : -4)),
    primaryColorPressed: seat(baseTone + (dark ? -4 : -8)),
    primaryColorSuppl: seat(baseTone),
  };
  const material = dark ? "rgba(38, 38, 43, 0.88)" : "rgba(255, 255, 255, 0.86)";
  // Base type nudged toward Miuix's 17px main size: desktop 14→15px, mobile stays 17px.
  const bodySize = mobile ? "1.0625rem" : "0.9375rem";
  const overrides: GlobalThemeOverrides = {
    common: {
      ...accent,
      fontFamily: "var(--font-family-ui)",
      fontFamilyMono: "var(--font-family-mono)",
      fontWeightStrong: "600",
      fontSize: bodySize,
      fontSizeTiny: "0.75rem",
      fontSizeSmall: mobile ? "0.9375rem" : "0.8125rem",
      fontSizeMedium: bodySize,
      fontSizeLarge: mobile ? "1.0625rem" : "1rem",
      lineHeight: "1.5",
      heightTiny: "24px",
      heightSmall: mobile ? "44px" : "28px",
      heightMedium: mobile ? "44px" : "32px",
      heightLarge: mobile ? "48px" : "40px",
      borderRadius: "var(--radius-control)",
      borderRadiusSmall: "var(--radius-xs)",
      textColorBase: primaryText,
      textColor1: primaryText,
      textColor2: primaryText,
      textColor3: tertiaryText,
      placeholderColor: tertiaryText,
      iconColor: secondaryText,
      iconColorHover: primaryText,
      borderColor: border,
      dividerColor: border,
      cardColor: surface,
      modalColor: elevated,
      popoverColor: elevated,
    },
    Button: {
      fontWeight: "500",
      textColorPrimary: textOnAccent(accent.primaryColor),
      textColorHoverPrimary: textOnAccent(accent.primaryColorHover),
      textColorPressedPrimary: textOnAccent(accent.primaryColorPressed),
      textColorFocusPrimary: textOnAccent(accent.primaryColor),
    },
    Card: { borderRadius: "var(--radius-panel)" },
    Dialog: { borderRadius: "var(--radius-panel)", titleFontWeight: "600" },
    Switch: {
      railBorderRadiusSmall: "999px",
      railBorderRadiusMedium: "999px",
      railBorderRadiusLarge: "999px",
      buttonBorderRadiusSmall: "999px",
      buttonBorderRadiusMedium: "999px",
      buttonBorderRadiusLarge: "999px",
    },
  };
  const variables: Record<string, string> = {
    "--text-primary": primaryText,
    "--text-secondary": secondaryText,
    "--text-tertiary": tertiaryText,
    "--surface-content": surface,
    "--surface-elevated": elevated,
    // A selected label needs a stable contrast even when its bar overlaps artwork.
    "--surface-selected": `color-mix(in srgb, ${accent.primaryColor} ${increasedContrast ? 6 : 12}%, ${elevated})`,
    "--border-color": border,
    "--material-regular-bg": increasedContrast ? elevated : material,
    "--material-solid-bg": elevated,
    "--material-border": border,
    "--material-highlight": dark ? "rgba(255, 255, 255, 0.12)" : "rgba(255, 255, 255, 0.8)",
    "--main-color": accent.primaryColor,
    "--main-second-color": `${accent.primaryColor}1f`,
    "--main-boxshadow-color": `${accent.primaryColor}26`,
    "--main-boxshadow-hover-color": `${accent.primaryColor}05`,
  };
  return { overrides, variables };
}
