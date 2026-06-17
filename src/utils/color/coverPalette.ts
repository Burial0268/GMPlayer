import ColorThief from "colorthief";
import {
  Hct,
  QuantizerCelebi,
  Score,
  themeFromSourceColor,
} from "@material/material-color-utilities";
import { settingStore, siteStore } from "@/store";

export type RGB = [number, number, number];
export type HSL = [number, number, number];

type BrowserColorThief = {
  getPalette(sourceImage: HTMLImageElement, colorCount?: number, quality?: number): RGB[] | null;
};

type MaterialPalette = {
  hue: number;
  chroma: number;
};

export interface CoverPalette {
  sourceColor: string;
  accentColor: string;
  panelAccentColor: string;
  secondaryColor: string;
  tertiaryColor: string;
  surfaceColor: string;
  buttonColor: string;
  onButtonColor: string;
  onAccentColor: string;
  gradient: string;
  panelGradient: string;
}

const DEFAULT_RGB: RGB = [128, 128, 128];
const DEFAULT_SOURCE_RGB: RGB = [98, 102, 116];
const PALETTE_CACHE = new Map<string, Promise<CoverPalette>>();

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));
const roundChannel = (value: number) => Math.round(clamp(value, 0, 255));

export const rgb2Hsl = ([r, g, b]: RGB): HSL => {
  r /= 255;
  g /= 255;
  b /= 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let h = 0;
  let s = 0;
  const l = (max + min) / 2;

  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0);
        break;
      case g:
        h = (b - r) / d + 2;
        break;
      case b:
        h = (r - g) / d + 4;
        break;
    }
    h /= 6;
  }

  return [h, s, l];
};

export const hsl2Rgb = ([h, s, l]: HSL): RGB => {
  let r: number;
  let g: number;
  let b: number;

  if (s === 0) {
    r = g = b = l;
  } else {
    const hue2rgb = (p: number, q: number, t: number): number => {
      if (t < 0) t += 1;
      if (t > 1) t -= 1;
      if (t < 1 / 6) return p + (q - p) * 6 * t;
      if (t < 1 / 2) return q;
      if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
      return p;
    };
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    r = hue2rgb(p, q, h + 1 / 3);
    g = hue2rgb(p, q, h);
    b = hue2rgb(p, q, h - 1 / 3);
  }

  return [roundChannel(r * 255), roundChannel(g * 255), roundChannel(b * 255)];
};

export const calcLuminance = (color: RGB): number => {
  const [r, g, b] = color.map((c) => {
    const channel = c / 255;
    return channel <= 0.03928 ? channel / 12.92 : Math.pow((channel + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

export const argb2Rgb = (argb: number): RGB => [
  (argb >> 16) & 0xff,
  (argb >> 8) & 0xff,
  argb & 0xff,
];

export const rgb2Argb = (r: number, g: number, b: number): number =>
  (0xff << 24) | (roundChannel(r) << 16) | (roundChannel(g) << 8) | roundChannel(b);

export const Rgb2Hex = (r: number, g: number, b: number): string =>
  `#${[r, g, b].map((c) => roundChannel(c).toString(16).padStart(2, "0")).join("")}`;

export const formatRgbTriplet = (rgb: RGB): string =>
  rgb.map((channel) => roundChannel(channel)).join(", ");

export const normalizeColor = (rgb: RGB): RGB => {
  const [h, initialS, initialL] = rgb2Hsl(rgb);
  if (Math.max(...rgb) - Math.min(...rgb) < 5) {
    return DEFAULT_SOURCE_RGB;
  }
  const s = clamp(initialS, 0.28, 0.86);
  const l = clamp(initialL, 0.36, 0.74);
  return hsl2Rgb([h, s, l]);
};

export const calcWhiteShadeColor = (rgb: RGB, amount = 0.5): RGB =>
  rgb.map((channel) => roundChannel(channel * (1 - amount) + 255 * amount)) as RGB;

const normalizeCoverUrl = (coverSrc: string): string => coverSrc.replace(/^http:/, "https:");

const tripletFromArgb = (argb: number): string => formatRgbTriplet(argb2Rgb(argb));

const argbFromTriplet = ([r, g, b]: RGB): number => rgb2Argb(r, g, b);

const loadImage = (coverSrc: string): Promise<HTMLImageElement> =>
  new Promise((resolve, reject) => {
    const image = new Image();
    image.crossOrigin = "Anonymous";
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Failed to load cover image: ${coverSrc}`));
    image.src = coverSrc;
  });

const getImagePixels = (image: HTMLImageElement): number[] => {
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;

  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return [];

  ctx.drawImage(image, 0, 0, image.naturalWidth, image.naturalHeight, 0, 0, size, size);
  const data = ctx.getImageData(0, 0, size, size).data;
  const pixels: number[] = [];

  for (let i = 0; i < data.length; i += 4) {
    const alpha = data[i + 3];
    if (alpha < 128) continue;
    pixels.push(
      (((alpha << 24) >>> 0) |
        ((data[i] << 16) >>> 0) |
        ((data[i + 1] << 8) >>> 0) |
        data[i + 2]) >>>
        0,
    );
  }

  return pixels;
};

const isLowChromaArgb = (argb: number): boolean => {
  const hct = Hct.fromInt(argb);
  return hct.chroma < 8 || Math.max(...argb2Rgb(argb)) - Math.min(...argb2Rgb(argb)) < 12;
};

const liftLowChromaSource = (argb: number): number => {
  const hct = Hct.fromInt(argb);
  if (!isLowChromaArgb(argb)) return argb;
  const hue = Number.isFinite(hct.hue) ? hct.hue : 260;
  return Hct.from(hue, 34, clamp(hct.tone, 42, 58)).toInt();
};

const getScoredSourceColor = (image: HTMLImageElement, fallbackPalette: RGB[]): number => {
  const pixels = getImagePixels(image);
  if (!pixels.length) return argbFromTriplet(fallbackPalette[0] ?? DEFAULT_SOURCE_RGB);

  const quantizedColors = QuantizerCelebi.quantize(pixels, 128);
  const ranked = Score.score(quantizedColors);
  if (ranked[0]) return liftLowChromaSource(ranked[0]);

  const fallback = fallbackPalette
    .map((rgb) => argbFromTriplet(normalizeColor(rgb)))
    .find((argb) => !isLowChromaArgb(argb));

  return liftLowChromaSource(fallback ?? argbFromTriplet(DEFAULT_SOURCE_RGB));
};

const getPreferredPalette = (palettes: Record<string, MaterialPalette>): MaterialPalette => {
  const requested = settingStore().colorType;
  return palettes[requested] ?? palettes.secondary ?? palettes.primary;
};

const tone = (palette: MaterialPalette, value: number, chromaBoost = 0): number =>
  Hct.from(palette.hue, Math.max(palette.chroma + chromaBoost, palette.chroma), value).toInt();

const rgba = (rgb: string, alpha: number): string => `rgba(${rgb}, ${alpha})`;
const rgbaVar = (rgb: string, varName: string, fallback: number): string =>
  `rgba(${rgb}, var(${varName}, ${fallback}))`;

const getGradientFromMonetPalette = (
  source: string,
  primary: string,
  secondary: string,
  tertiary: string,
  dark: string,
): string =>
  `linear-gradient(-45deg, rgb(${dark}) 0%, rgb(${source}) 28%, rgb(${primary}) 52%, rgb(${secondary}) 74%, rgb(${tertiary}) 100%)`;

export const getGradientFromPalette = (palette: RGB[]): string => {
  const colors = palette
    .map((rgb) => normalizeColor(rgb))
    .sort((a, b) => rgb2Hsl(b)[1] - rgb2Hsl(a)[1])
    .slice(0, 5);

  if (!colors.length) {
    return getFallbackPalette().gradient;
  }

  return `linear-gradient(-45deg, ${colors.map((rgb) => `rgb(${formatRgbTriplet(rgb)})`).join(", ")})`;
};

const createCoverPalette = (sourceArgb: number): CoverPalette => {
  const theme = themeFromSourceColor(sourceArgb);
  const palettes = theme.palettes as unknown as Record<string, MaterialPalette>;
  const selected = getPreferredPalette(palettes);
  const primary = palettes.primary;
  const secondary = palettes.secondary;
  const tertiary = palettes.tertiary;
  const neutral = palettes.neutral;

  const sourceColor = tripletFromArgb(sourceArgb);
  const accentColor = tripletFromArgb(tone(selected, 88, 8));
  const panelAccentArgb = tone(selected, 46, 22);
  const buttonArgb = tone(selected, 84, 10);
  const panelAccentColor = tripletFromArgb(panelAccentArgb);
  const secondaryColor = tripletFromArgb(tone(secondary, 50, 14));
  const tertiaryColor = tripletFromArgb(tone(tertiary, 48, 16));
  const surfaceColor = tripletFromArgb(tone(neutral, 94));
  const buttonColor = tripletFromArgb(buttonArgb);
  const onButtonColor = calcLuminance(argb2Rgb(buttonArgb)) > 0.48 ? "18, 18, 22" : "255, 255, 255";
  const onAccentColor =
    calcLuminance(argb2Rgb(panelAccentArgb)) > 0.4 ? "20, 20, 24" : "255, 255, 255";
  const dark = tripletFromArgb(tone(neutral, 18));
  const primaryColor = tripletFromArgb(tone(primary, 48, 16));

  return {
    sourceColor,
    accentColor,
    panelAccentColor,
    secondaryColor,
    tertiaryColor,
    surfaceColor,
    buttonColor,
    onButtonColor,
    onAccentColor,
    gradient: getGradientFromMonetPalette(
      sourceColor,
      primaryColor,
      secondaryColor,
      tertiaryColor,
      dark,
    ),
    panelGradient: [
      "linear-gradient(180deg, transparent 0%, transparent 54%, var(--content-panel-gradient-overlay, transparent) 100%)",
      `radial-gradient(ellipse 82% 58% at 50% 22%, ${rgbaVar(
        panelAccentColor,
        "--content-panel-hero-wash-opacity",
        0.36,
      )} 0%, ${rgbaVar(panelAccentColor, "--content-panel-mid-wash-opacity", 0.22)} 42%, transparent 78%)`,
      `radial-gradient(circle at 50% 24%, ${rgbaVar(
        secondaryColor,
        "--content-panel-side-wash-opacity",
        0.15,
      )} 0%, transparent 46%)`,
      `radial-gradient(ellipse 118% 76% at 50% 24%, ${rgbaVar(
        tertiaryColor,
        "--content-panel-wash-opacity",
        0.12,
      )} 0%, transparent 82%)`,
    ].join(", "),
  };
};

const getFallbackPalette = (): CoverPalette => createCoverPalette(argbFromTriplet(DEFAULT_RGB));

const extractCoverPalette = async (image: HTMLImageElement): Promise<CoverPalette> => {
  const ColorThiefCtor = ColorThief as unknown as { new (): BrowserColorThief };
  const colorThief = new ColorThiefCtor();
  const fallbackPalette = (await colorThief.getPalette(image, 12, 6)) ?? [];
  const sourceArgb = getScoredSourceColor(image, fallbackPalette);
  return createCoverPalette(sourceArgb);
};

export const getCoverPalette = (coverSrc: string): Promise<CoverPalette> => {
  if (!coverSrc) return Promise.resolve(getFallbackPalette());

  const normalizedSrc = normalizeCoverUrl(coverSrc);
  const cached = PALETTE_CACHE.get(normalizedSrc);
  if (cached) return cached;

  const request = loadImage(normalizedSrc)
    .then(extractCoverPalette)
    .catch((error) => {
      console.error("Cover palette extraction failed:", error);
      return getFallbackPalette();
    });

  PALETTE_CACHE.set(normalizedSrc, request);
  return request;
};

export const applyGlobalCoverPalette = async (coverSrc: string): Promise<CoverPalette> => {
  const palette = await getCoverPalette(coverSrc);
  const site = siteStore();
  site.songPicColor = palette.accentColor;
  site.songPicGradient = palette.gradient;
  return palette;
};

export const getCoverColor = (coverSrc: string): Promise<string> =>
  applyGlobalCoverPalette(coverSrc).then((palette) => palette.gradient);
