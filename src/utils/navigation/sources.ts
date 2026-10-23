/// <reference lib="es2021.weakref" />
import { nextLayerId, remember, type LayerSource, type SourceKind } from "./layers";
import type { InjectionKey } from "vue";
import { captureDecorations, type SourceDecoration } from "./sourceAppearance";
import type { CoverShape } from "./surfaceProjection";

export interface SurfaceRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SourceGeometry {
  surface: SurfaceRect;
  cover?: SurfaceRect;
  radius: number;
  coverRadius: number;
  coverMode: "crop" | "image";
  image?: string;
  imageRect?: SurfaceRect;
  imageSize?: { width: number; height: number };
  artworks: ArtworkGeometry[];
  fit: string;
  position: string;
  title?: TitleGeometry;
  decorations: SourceDecoration[];
}

export interface ArtworkGeometry {
  src: string;
  rect: SurfaceRect;
  size: { width: number; height: number };
  opacity: number;
  shape?: CoverShape;
}

export interface TitleGeometry {
  rect: SurfaceRect;
  text: string;
  fontFamily: string;
  fontSize: string;
  fontWeight: string;
  lineHeight: string;
  letterSpacing: string;
  color: string;
  whiteSpace: string;
  textAlign: string;
  lineClamp: string;
}

export const pageSourceKey: InjectionKey<SourceGeometry | undefined> = Symbol("page-source");

export function measureTitle(element: HTMLElement): TitleGeometry | undefined {
  const title = element.matches("[data-navigation-title]")
    ? element
    : [...element.querySelectorAll<HTMLElement>("[data-navigation-title]")].find(
        (node) => node.getClientRects().length,
      );
  if (!title?.textContent?.trim()) return;
  const style = getComputedStyle(title);
  return {
    rect: rectOf(title),
    text: title.textContent.trim(),
    fontFamily: style.fontFamily,
    fontSize: style.fontSize,
    fontWeight: style.fontWeight,
    lineHeight: style.lineHeight,
    letterSpacing: style.letterSpacing,
    color: style.color,
    whiteSpace: style.whiteSpace,
    textAlign: style.textAlign,
    lineClamp: style.webkitLineClamp,
  };
}

const radiusOf = (element: Element) => {
  const { width, height } = element.getBoundingClientRect();
  const value = getComputedStyle(element).borderTopLeftRadius;
  const radius =
    Number.parseFloat(value) * (value.endsWith("%") ? Math.min(width, height) / 100 : 1);
  return Math.min(radius || 0, width / 2, height / 2);
};

export function imageShapeOf(element: HTMLImageElement): CoverShape {
  const rect = rectOf(element);
  const style = getComputedStyle(element);
  const value = style.borderTopLeftRadius;
  const fraction = value.endsWith("%")
    ? Number.parseFloat(value) / 100
    : Number.parseFloat(value) /
      Math.min(Number.parseFloat(style.width), Number.parseFloat(style.height));
  return { rect, radius: Math.min(fraction, 0.5) * Math.min(rect.width, rect.height) };
}

interface SourceRecord {
  element: WeakRef<HTMLElement>;
  focus: WeakRef<HTMLElement>;
  identity?: string;
  width: number;
  snapshot: SourceGeometry;
}

const sources = new Map<string, SourceRecord>();
let restoringFocus = false;

export const isRestoringSourceFocus = () => restoringFocus;

export function rectOf(element: Element): SurfaceRect {
  const rect = element.getBoundingClientRect();
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
}

export function isVisibleRect(rect: SurfaceRect): boolean {
  return (
    rect.width > 1 &&
    rect.height > 1 &&
    rect.x < window.innerWidth &&
    rect.y < window.innerHeight &&
    rect.x + rect.width > 0 &&
    rect.y + rect.height > 0
  );
}

/** The painted pixels, including object-fit cropping and the image's hover transform. */
export function imagePixelRect(
  box: SurfaceRect,
  size: { width: number; height: number },
  fit: string,
  position: string,
): SurfaceRect {
  if (fit !== "cover" && fit !== "contain") return box;
  const scale = (fit === "cover" ? Math.max : Math.min)(
    box.width / size.width,
    box.height / size.height,
  );
  const width = size.width * scale;
  const height = size.height * scale;
  const [x, y] = position.split(" ");
  const offset = (space: number, value: string) =>
    value.endsWith("%") ? (space * Number.parseFloat(value)) / 100 : Number.parseFloat(value);
  return {
    x: box.x + offset(box.width - width, x),
    y: box.y + offset(box.height - height, y),
    width,
    height,
  };
}

export function measureGeometry(
  element: HTMLElement,
  fallbackImageSize?: SourceGeometry["imageSize"],
): SourceGeometry {
  const cover = element.matches("[data-navigation-cover]")
    ? element
    : element.querySelector<HTMLElement>("[data-navigation-cover]");
  const imageRoot = cover ?? element;
  const image =
    imageRoot instanceof HTMLImageElement
      ? imageRoot
      : (imageRoot.querySelector<HTMLImageElement>("img[data-navigation-shared-image]") ??
        imageRoot.querySelector<HTMLImageElement>("img:not([aria-hidden='true'])"));
  const imageOwned = image?.hasAttribute("data-navigation-shared-image") ?? false;
  const artwork = imageOwned ? image : (cover ?? image);
  const imageStyle = image ? getComputedStyle(image) : undefined;
  const imageSize = image?.naturalWidth
    ? { width: image.naturalWidth, height: image.naturalHeight }
    : fallbackImageSize;
  const marked = [...imageRoot.querySelectorAll<HTMLImageElement>("img[data-navigation-artwork]")];
  const artworks = (marked.length ? marked : image ? [image] : []).flatMap((node) => {
    const style = getComputedStyle(node);
    const opacity = Number(style.opacity);
    if (!node.complete || !node.naturalWidth || opacity === 0) return [];
    const size = { width: node.naturalWidth, height: node.naturalHeight };
    return [
      {
        src: node.currentSrc || node.src,
        rect: imagePixelRect(rectOf(node), size, style.objectFit, style.objectPosition),
        size,
        opacity,
        shape: node.hasAttribute("data-navigation-shared-image") ? imageShapeOf(node) : undefined,
      },
    ];
  });
  const primary = artworks.reduce<ArtworkGeometry | undefined>(
    (best, value) => (!best || value.opacity >= best.opacity ? value : best),
    undefined,
  );
  return {
    surface: rectOf(element),
    cover: artwork ? rectOf(artwork) : undefined,
    radius: radiusOf(element),
    coverRadius: imageOwned ? imageShapeOf(image!).radius : artwork ? radiusOf(artwork) : 0,
    coverMode: imageOwned ? "image" : "crop",
    image: primary?.src,
    imageSize: primary?.size ?? imageSize,
    artworks,
    imageRect:
      primary?.rect ??
      (image && imageStyle && imageSize
        ? imagePixelRect(rectOf(image), imageSize, imageStyle.objectFit, imageStyle.objectPosition)
        : undefined),
    fit: imageStyle?.objectFit === "contain" ? "contain" : "cover",
    position: imageStyle?.objectPosition ?? "50% 50%",
    title: measureTitle(element),
    decorations: captureDecorations(element),
  };
}

export function captureSource(
  origin: Event | HTMLElement | undefined,
  ownerId: string,
  kind: SourceKind = "control",
  identity?: string,
): LayerSource | undefined {
  if (typeof HTMLElement === "undefined") return;
  const element = origin instanceof HTMLElement ? origin : origin?.currentTarget;
  if (!(element instanceof HTMLElement) || !element.isConnected) return;
  const snapshot = measureGeometry(element);
  if (!isVisibleRect(snapshot.surface)) return;
  const id = nextLayerId();
  const focused = document.activeElement;
  element.dataset.navigationSource = id;
  remember(sources, id, {
    element: new WeakRef(element),
    focus: new WeakRef(
      focused instanceof HTMLElement && element.contains(focused) ? focused : element,
    ),
    identity: identity ?? element.dataset.navigationIdentity,
    width: window.innerWidth,
    snapshot,
  });
  return { id, ownerId, kind, identity };
}

export function sourceSnapshot(source?: LayerSource): SourceGeometry | undefined {
  return source ? sources.get(source.id)?.snapshot : undefined;
}

export function liveSource(
  source?: LayerSource,
): { element: HTMLElement; geometry: SourceGeometry } | undefined {
  if (!source) return;
  const record = sources.get(source.id);
  const element = record?.element.deref();
  if (
    !record ||
    !element?.isConnected ||
    element.dataset.navigationSource !== source.id ||
    Math.abs(record.width - window.innerWidth) > 1 ||
    (record.identity && element.dataset.navigationIdentity !== record.identity)
  )
    return;
  const geometry = measureGeometry(element);
  if (!isVisibleRect(geometry.surface)) return;
  return { element, geometry };
}

export function restoreSourceFocus(source?: LayerSource): boolean {
  const record = source ? sources.get(source.id) : undefined;
  const element = record?.focus.deref();
  if (!element?.isConnected || element.closest("[inert]")) return false;
  const temporary =
    !element.hasAttribute("tabindex") && !element.matches("button,a,input,select,textarea");
  if (temporary) element.setAttribute("tabindex", "-1");
  // Restoring an input's focus must not activate its layer again.
  restoringFocus = true;
  try {
    element.focus({ preventScroll: true });
  } finally {
    restoringFocus = false;
  }
  if (temporary)
    element.addEventListener("blur", () => element.removeAttribute("tabindex"), { once: true });
  return document.activeElement === element;
}
