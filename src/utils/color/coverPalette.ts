import ColorThief from "colorthief";
import { Hct, themeFromSourceColor } from "@material/material-color-utilities";
import { settingStore, siteStore } from "@/store";
import { isAssetUrl } from "@/utils/coverUrl";
import { quantizeCoverPixels } from "./quantizeClient";

export type RGB = [number, number, number];
export type HSL = [number, number, number];

type BrowserColorThief = {
  getPalette(sourceImage: ImageData, colorCount?: number, quality?: number): RGB[] | null;
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
// LRU-capped: every played track and every visited album/playlist/artist page
// inserts an entry, so an uncapped map grows for the whole session.
const PALETTE_CACHE = new Map<string, Promise<CoverPalette>>();
const PALETTE_CACHE_MAX_ENTRIES = 96;

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

/**
 * The cache key and load URL for a cover.
 *
 * Skips the `http:` → `https:` upgrade for an asset-protocol URL: on Windows and
 * Android `convertFileSrc` yields `http://asset.localhost/…`, and rewriting that
 * points at an origin with no protocol handler — the image then fails to load and
 * every local track silently falls back to the default palette.
 */
const normalizeCoverUrl = (coverSrc: string): string =>
  isAssetUrl(coverSrc) ? coverSrc : coverSrc.replace(/^http:/, "https:");

// Palette extraction only ever samples a 64x64 grid, so there is no reason to
// pull the original artwork. NCM's CDN resizes server-side via `param`, which
// turns a ~1400x1400 original (≈7.8 MB once decoded) into ≈256 KB. Restricted
// to the NCM hosts: other CDNs may sign their URLs, and an extra query param
// would break the signature.
const PALETTE_SOURCE_SIZE = 256;
const NCM_IMAGE_HOST_REGEX = /(^|\.)music\.12[67]\.net$/;

const toPaletteSourceUrl = (url: string): string => {
  if (!/^https?:/i.test(url)) return url;
  try {
    const parsed = new URL(url);
    if (!NCM_IMAGE_HOST_REGEX.test(parsed.hostname)) return url;
    parsed.searchParams.set("param", `${PALETTE_SOURCE_SIZE}y${PALETTE_SOURCE_SIZE}`);
    return parsed.toString();
  } catch {
    return url;
  }
};

// Sampling grid for quantization. Kept at 64 so the scored source color matches
// what this module produced before the full-size decode was removed.
const SAMPLE_SIZE = 64;

// One reusable scratch canvas for every extraction. Creating one per cover left
// a full-size backing store per played track in the renderer's non-JS memory,
// which the heap snapshot cannot see and Chromium reclaims only lazily.
let sampleCanvas: HTMLCanvasElement | null = null;
let sampleCtx: CanvasRenderingContext2D | null = null;

const getSampleContext = (): CanvasRenderingContext2D | null => {
  if (sampleCtx) return sampleCtx;
  sampleCanvas ??= document.createElement("canvas");
  sampleCanvas.width = SAMPLE_SIZE;
  sampleCanvas.height = SAMPLE_SIZE;
  sampleCtx = sampleCanvas.getContext("2d", { willReadFrequently: true });
  return sampleCtx;
};

/**
 * Downscale a cover into the shared scratch canvas and return a detached
 * snapshot of its pixels.
 *
 * Returning ImageData (rather than the canvas) is what makes the shared canvas
 * safe: concurrent extractions each get their own snapshot, and ColorThief
 * accepts ImageData directly — so it never builds a natural-size canvas of its
 * own, which was the single largest per-track allocation here.
 */
const sampleCoverImage = (image: HTMLImageElement): ImageData | null => {
  const ctx = getSampleContext();
  if (!ctx) return null;

  const width = image.naturalWidth || image.width;
  const height = image.naturalHeight || image.height;
  if (!width || !height) return null;

  ctx.clearRect(0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
  ctx.drawImage(image, 0, 0, width, height, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
  return ctx.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
};

const tripletFromArgb = (argb: number): string => formatRgbTriplet(argb2Rgb(argb));

const argbFromTriplet = ([r, g, b]: RGB): number => rgb2Argb(r, g, b);

const loadImage = (coverSrc: string): Promise<HTMLImageElement> =>
  new Promise((resolve, reject) => {
    const image = new Image();
    image.crossOrigin = "Anonymous";
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Failed to load cover image: ${coverSrc}`));
    image.src = toPaletteSourceUrl(coverSrc);
  });

/**
 * Opaque pixels as packed `0xAARRGGBB`, in a transferable buffer.
 *
 * `Uint32Array` rather than `number[]` because this crosses to the quantize
 * worker by transfer, and a plain array would be structured-cloned instead —
 * 4096 boxed numbers copied per cover, which is exactly the kind of cost the
 * worker exists to remove.
 */
const getImagePixels = (sample: ImageData): Uint32Array => {
  const data = sample.data;
  const pixels = new Uint32Array(data.length >> 2);
  let count = 0;

  for (let i = 0; i < data.length; i += 4) {
    const alpha = data[i + 3];
    if (alpha < 128) continue;
    pixels[count++] =
      (((alpha << 24) >>> 0) |
        ((data[i] << 16) >>> 0) |
        ((data[i + 1] << 8) >>> 0) |
        data[i + 2]) >>>
      0;
  }

  // Exact length: the worker transfers this buffer, so trailing zeros would be
  // quantized as opaque black.
  return pixels.subarray(0, count).slice();
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

/**
 * Score the cover's source colour.
 *
 * Async because the quantizer runs in a worker — it is 30–140 ms of Wu box
 * splitting, and on a playlist page it used to land in the same frame as the
 * first rows. Everything else here is a few milliseconds and stays inline.
 */
const getScoredSourceColor = async (sample: ImageData, fallbackPalette: RGB[]): Promise<number> => {
  const pixels = getImagePixels(sample);
  if (!pixels.length) return argbFromTriplet(fallbackPalette[0] ?? DEFAULT_SOURCE_RGB);

  const { argb } = await quantizeCoverPixels(pixels, 128);
  if (argb !== null) return liftLowChromaSource(argb);

  const fallback = fallbackPalette
    .map((rgb) => argbFromTriplet(normalizeColor(rgb)))
    .find((candidate) => !isLowChromaArgb(candidate));

  return liftLowChromaSource(fallback ?? argbFromTriplet(DEFAULT_SOURCE_RGB));
};

const getPreferredPalette = (palettes: Record<string, MaterialPalette>): MaterialPalette => {
  const requested = settingStore().colorType;
  return palettes[requested] ?? palettes.secondary ?? palettes.primary;
};

const tone = (palette: MaterialPalette, value: number, chromaBoost = 0): number =>
  Hct.from(palette.hue, Math.max(palette.chroma + chromaBoost, palette.chroma), value).toInt();

// 在指定 hue/chroma 上取某个明度(tone)的颜色——用于构建「同色相、不同明度」的统一强调色阶梯
const toneAt = (hue: number, chroma: number, value: number): number =>
  Hct.from(hue, chroma, value).toInt();

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

// 浅色面板上的「舞台」色洗：单色相、柔和、顶部居中——对桌面(封面在左上)与移动端(封面居中靠上)
// 都自然对齐；底部回落到面板底色以保证文字可读。摒弃旧的 source/secondary/tertiary 多层叠加(发「脏」)。
const getPanelStageGradient = (panelAccentColor: string): string =>
  [
    `radial-gradient(125% 78% at 50% 0%, ${rgbaVar(
      panelAccentColor,
      "--content-panel-hero-wash-opacity",
      0.16,
    )} 0%, ${rgbaVar(panelAccentColor, "--content-panel-mid-wash-opacity", 0.06)} 40%, transparent 74%)`,
    "linear-gradient(180deg, transparent 0%, transparent 62%, var(--content-panel-gradient-overlay, transparent) 100%)",
  ].join(", ");

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
  const secondary = palettes.secondary;
  const tertiary = palettes.tertiary;
  const neutral = palettes.neutral;

  // 统一色相 + 受控色度：所有强调色共享同一 hue/chroma，仅在明度(tone)上分档，
  // 让深浅版本看起来是「同一种颜色」而非彼此割裂；色度封顶避免低透明度叠加时发灰发「脏」。
  const hue = Number.isFinite(selected.hue) ? selected.hue : 260;
  const chroma = clamp(selected.chroma, 30, 56);

  // 亮调：深色/沉浸表面(歌词、迷你播放器、托盘、大播放器取色) | 暗调：浅色面板标题/描边/色洗 | 按钮主色
  const accentArgb = toneAt(hue, chroma, 72);
  const panelAccentArgb = toneAt(hue, chroma, 46);
  const buttonArgb = toneAt(hue, chroma, 50);

  const sourceColor = tripletFromArgb(sourceArgb);
  const accentColor = tripletFromArgb(accentArgb);
  const panelAccentColor = tripletFromArgb(panelAccentArgb);
  const buttonColor = tripletFromArgb(buttonArgb);
  // 副/三级色仍保留各自色相(用于沉浸渐变)，但同样封顶色度；不再参与浅色面板色洗
  const secondaryColor = tripletFromArgb(
    toneAt(secondary.hue, clamp(secondary.chroma, 22, 48), 52),
  );
  const tertiaryColor = tripletFromArgb(toneAt(tertiary.hue, clamp(tertiary.chroma, 22, 48), 50));
  const surfaceColor = tripletFromArgb(tone(neutral, 94));
  const onButtonColor = calcLuminance(argb2Rgb(buttonArgb)) > 0.55 ? "18, 18, 22" : "255, 255, 255";
  const onAccentColor =
    calcLuminance(argb2Rgb(panelAccentArgb)) > 0.45 ? "20, 20, 24" : "255, 255, 255";
  const dark = tripletFromArgb(tone(neutral, 18));
  const primaryColor = tripletFromArgb(toneAt(hue, chroma, 48));

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
    panelGradient: getPanelStageGradient(panelAccentColor),
  };
};

const getFallbackPalette = (): CoverPalette => createCoverPalette(argbFromTriplet(DEFAULT_RGB));

const extractCoverPalette = async (image: HTMLImageElement): Promise<CoverPalette> => {
  const sample = sampleCoverImage(image);

  // The sample is a detached snapshot, so the element has served its purpose.
  // Dropping src/handlers here releases the element's hold on the decoded frame
  // immediately instead of waiting for a GC that — seeing only a few hundred
  // bytes of JS — has no reason to feel urgency about the bitmap behind it.
  image.onload = null;
  image.onerror = null;
  image.src = "";

  if (!sample) return getFallbackPalette();

  const ColorThiefCtor = ColorThief as unknown as { new (): BrowserColorThief };
  const colorThief = new ColorThiefCtor();
  // Feed the downscaled snapshot, not the image element: ColorThief's
  // HTMLImageElement path allocates a canvas at natural size and reads the
  // whole thing back. Its result only seeds edge-case fallbacks below, so it
  // never justified a full-resolution pass.
  const fallbackPalette = colorThief.getPalette(sample, 12, 6) ?? [];
  const sourceArgb = await getScoredSourceColor(sample, fallbackPalette);
  return createCoverPalette(sourceArgb);
};

export const getCoverPalette = (coverSrc: string): Promise<CoverPalette> => {
  if (!coverSrc) return Promise.resolve(getFallbackPalette());

  const normalizedSrc = normalizeCoverUrl(coverSrc);
  const cached = PALETTE_CACHE.get(normalizedSrc);
  if (cached) {
    // Refresh recency so hot covers survive eviction
    PALETTE_CACHE.delete(normalizedSrc);
    PALETTE_CACHE.set(normalizedSrc, cached);
    return cached;
  }

  const request = loadImage(normalizedSrc)
    .then(extractCoverPalette)
    .catch((error) => {
      console.error("Cover palette extraction failed:", error);
      return getFallbackPalette();
    });

  while (PALETTE_CACHE.size >= PALETTE_CACHE_MAX_ENTRIES) {
    const oldest = PALETTE_CACHE.keys().next().value;
    if (oldest === undefined) break;
    PALETTE_CACHE.delete(oldest);
  }
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
