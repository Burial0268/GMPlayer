import { animateMini } from "motion-v";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { layerMotion } from "./motion";
import {
  liveSource,
  imagePixelRect,
  measureGeometry,
  measureTitle,
  rectOf,
  sourceSnapshot,
  imageShapeOf,
  type SurfaceRect,
  type ArtworkGeometry,
  type TitleGeometry,
} from "./sources";
import type { LayerTransition } from "./layers";
import {
  cropClip,
  projectedCoverFrames,
  projectedSurfaceClips,
  sharedImageFrames,
  type SurfaceScale,
} from "./surfaceProjection";
import { observeSource } from "./sourceLifecycle";
import {
  applyDecorationPaint,
  decorationRect,
  type InterruptedDecoration,
} from "./sourceAppearance";

export interface InterruptedSurface {
  element: HTMLElement;
  transform: string;
  contentOpacity: string;
  backdropOpacity?: number;
  elevationOpacity?: number;
  cover?: SurfaceRect;
  coverRadius?: number;
  artworks?: ArtworkGeometry[];
  titles?: { geometry: TitleGeometry; opacity: number }[];
  scale: SurfaceScale;
  decorations?: InterruptedDecoration[];
}

const paintedCrop = (element: HTMLElement): SurfaceRect => {
  const bounds = rectOf(element);
  const inset = getComputedStyle(element).clipPath.match(/^inset\((.*?)(?: round|\))/)?.[1];
  if (!inset) return bounds;
  const edges = inset.split(" ").map(Number.parseFloat);
  const [top, right = top, bottom = top, left = right] = edges;
  return {
    x: bounds.x + left,
    y: bounds.y + top,
    width: bounds.width - left - right,
    height: bounds.height - top - bottom,
  };
};

const sameTitleLayout = (a: TitleGeometry, b: TitleGeometry) =>
  a.fontFamily === b.fontFamily &&
  a.fontSize === b.fontSize &&
  a.fontWeight === b.fontWeight &&
  a.lineHeight === b.lineHeight &&
  a.letterSpacing === b.letterSpacing &&
  a.color === b.color &&
  a.whiteSpace === b.whiteSpace &&
  a.textAlign === b.textAlign &&
  a.lineClamp === b.lineClamp &&
  Math.abs(a.rect.width - b.rect.width) < 0.1;

export function animateRouteLayers(options: {
  incoming: HTMLElement;
  outgoing?: HTMLElement;
  host: HTMLElement;
  transition: LayerTransition;
  interrupted?: InterruptedSurface;
  complete: () => void;
}) {
  const { incoming, outgoing, host, transition, interrupted, complete } = options;
  const back = transition.direction === "pop";
  const foreground = back && outgoing ? outgoing : incoming;
  const previous = interrupted?.element === foreground ? interrupted : undefined;
  const content = foreground.querySelector<HTMLElement>(":scope > .route-surface-content");
  const styles = new Map<HTMLElement, Map<string, { value: string; priority: string }>>();
  const cleanups: (() => void)[] = [];
  const animations: ReturnType<typeof animateMini>[] = [];
  let coverProxy: HTMLElement | undefined;
  let backdrop: HTMLElement | undefined;
  let elevation: HTMLElement | undefined;
  let coverRadii: string[] | undefined;
  const artworkProxies: { element: HTMLImageElement; artwork: ArtworkGeometry }[] = [];
  const titleProxies: HTMLElement[] = [];
  const decorationProxies: { element: HTMLElement; key: string }[] = [];
  let finished = false;

  const save = (element: HTMLElement, ...properties: string[]) => {
    if (!styles.has(element)) styles.set(element, new Map());
    const saved = styles.get(element)!;
    for (const property of properties) {
      const name = property.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
      if (!saved.has(name))
        saved.set(name, {
          value: element.style.getPropertyValue(name),
          priority: element.style.getPropertyPriority(name),
        });
    }
  };
  const animate = (
    element: HTMLElement,
    keyframes: Parameters<typeof animateMini>[1],
    duration: number,
    delay = 0,
    spatial = false,
  ) => {
    save(element, ...Object.keys(keyframes));
    animations.push(
      animateMini(element, keyframes, {
        duration,
        delay,
        ease: spatial ? layerMotion.sharedEase : layerMotion.ease,
      }),
    );
  };
  const cleanup = () => {
    if (finished) return;
    finished = true;
    animations.forEach((animation) => animation.cancel());
    styles.forEach((properties, element) => {
      for (const [name, { value, priority }] of properties) {
        if (value) element.style.setProperty(name, value, priority);
        else element.style.removeProperty(name);
      }
    });
    cleanups.forEach((fn) => fn());
    complete();
  };

  if (prefersReducedMotion() || transition.direction === "none") {
    queueMicrotask(cleanup);
    return { cancel: cleanup, snapshot: () => undefined };
  }

  const hostRect = rectOf(host);
  const surfaceRect = rectOf(foreground);
  const viewport: SurfaceRect = {
    x: 0,
    y: 0,
    width: window.innerWidth,
    height: window.innerHeight,
  };
  const live = liveSource(transition.source);
  const source = back ? live?.geometry : sourceSnapshot(transition.source);
  const shared =
    source &&
    transition.source?.kind !== "control" &&
    (transition.direction === "push" || transition.direction === "pop");
  const prepareSurface = () => {
    save(
      foreground,
      "isolation",
      "--route-surface-top",
      "--route-surface-left",
      "--route-surface-width",
      "--route-surface-height",
    );
    foreground.style.isolation = "isolate";
    // The page's gutters and its loading height never bound the moving surface.
    foreground.style.setProperty("--route-surface-top", `${-surfaceRect.y}px`);
    foreground.style.setProperty("--route-surface-left", `${-surfaceRect.x}px`);
    foreground.style.setProperty("--route-surface-width", `${viewport.width}px`);
    foreground.style.setProperty("--route-surface-height", `${viewport.height}px`);
    foreground.dataset.navigationSurface = "";
    cleanups.push(() => {
      delete foreground.dataset.navigationSurface;
    });
  };
  const proxy = (className: string, rect: SurfaceRect, zIndex = "4", container?: HTMLElement) => {
    const element = document.createElement("div");
    const origin = container ? rectOf(container) : hostRect;
    element.className = className;
    element.setAttribute("aria-hidden", "true");
    Object.assign(element.style, {
      position: "absolute",
      left: "0",
      top: "0",
      margin: "0",
      width: `${rect.width}px`,
      height: `${rect.height}px`,
      transform: `translate3d(${rect.x - origin.x}px, ${rect.y - origin.y}px, 0)`,
      pointerEvents: "none",
      zIndex,
      contain: "layout paint style",
    });
    (container ?? host).append(element);
    cleanups.push(() => element.remove());
    return element;
  };
  const moveTitle = (element: HTMLElement, start: SurfaceRect, end: SurfaceRect) => {
    animate(
      element,
      {
        transform: [
          `translate3d(${start.x - hostRect.x}px, ${start.y - hostRect.y}px, 0)`,
          `translate3d(${end.x - hostRect.x}px, ${end.y - hostRect.y}px, 0)`,
        ],
      },
      layerMotion.shared,
      0,
      true,
    );
  };

  if (transition.to.at(-1)?.presentation !== "page") {
    save(incoming, "opacity");
    incoming.style.opacity = "0";
  }

  if (shared) {
    // Project the whole surface like the App Store card. Artwork and title have
    // their own geometry, so neither inherits the page's non-uniform scale.
    const detailCover = foreground.querySelector<HTMLElement>('[data-navigation-cover="page"]');
    const detail = detailCover ? measureGeometry(detailCover, source.imageSize) : undefined;
    const detailHeading = foreground.querySelector<HTMLElement>('[data-navigation-title="page"]');
    const detailTitle = detailHeading ? measureTitle(detailHeading) : undefined;
    const card =
      transition.source?.kind === "cover" ? (source.cover ?? source.surface) : source.surface;
    const sx = card.width / viewport.width;
    const sy = card.height / viewport.height;
    const radius = source.radius || source.coverRadius;
    const collapsedScale = { x: sx, y: sy, radiusX: radius, radiusY: radius };
    const expandedScale = { x: 1, y: 1, radiusX: 0, radiusY: 0 };
    const startScale = previous?.scale ?? (back ? expandedScale : collapsedScale);
    const endScale = back ? collapsedScale : expandedScale;
    const collapsed = `translate3d(${card.x}px, ${card.y}px, 0px) scale(${sx}, ${sy})`;
    const expanded = "translate3d(0px, 0px, 0px) scale(1, 1)";
    prepareSurface();
    save(foreground, "z-index", "transform-origin", "will-change");
    foreground.style.zIndex = "3";
    foreground.style.transformOrigin = `${-surfaceRect.x}px ${-surfaceRect.y}px`;
    foreground.style.willChange = "transform";
    animate(
      foreground,
      {
        transform: [
          previous?.transform ?? (back ? expanded : collapsed),
          back ? collapsed : expanded,
        ],
        clipPath: projectedSurfaceClips(viewport, surfaceRect, startScale, endScale),
      },
      layerMotion.shared,
      0,
      true,
    );

    const scrim = proxy("navigation-scrim", viewport, "2");
    backdrop = scrim;
    scrim.dataset.navigationBase = "";
    scrim.style.background = "var(--app-shell-bg, #fff)";
    // Coverage belongs to the page transition, regardless of artwork or card type.
    animate(scrim, { opacity: [previous?.backdropOpacity ?? 1, back ? 0 : 1] }, layerMotion.shared);

    const matrix = previous ? new DOMMatrixReadOnly(previous.transform) : undefined;
    const fromSurface = matrix
      ? {
          x: matrix.e,
          y: matrix.f,
          width: viewport.width * startScale.x,
          height: viewport.height * startScale.y,
        }
      : back
        ? viewport
        : card;
    const toSurface = back ? card : viewport;
    elevation = proxy("navigation-surface-elevation", fromSurface, "3");
    Object.assign(elevation.style, {
      boxSizing: "border-box",
      border: "1px solid var(--border-strong)",
      boxShadow: "var(--shadow-3)",
      contain: "layout style",
    });
    const surfacePosition = (rect: SurfaceRect) =>
      `translate3d(${rect.x - hostRect.x}px, ${rect.y - hostRect.y}px, 0)`;
    // Keep the edge in screen pixels; scaling a shadow stretches it with the page.
    animate(
      elevation,
      {
        transform: [surfacePosition(fromSurface), surfacePosition(toSurface)],
        width: [fromSurface.width, toSurface.width],
        height: [fromSurface.height, toSurface.height],
        borderRadius: [
          `${startScale.radiusX}px / ${startScale.radiusY}px`,
          `${endScale.radiusX}px / ${endScale.radiusY}px`,
        ],
        opacity: [previous?.elevationOpacity ?? 0, 1, 0],
      },
      layerMotion.shared,
      0,
      true,
    );

    if (content) {
      animate(
        content,
        { opacity: [Number(previous?.contentOpacity ?? (back ? 1 : 0)), back ? 0 : 1] },
        layerMotion.shared * 0.55,
        back ? 0 : layerMotion.shared * 0.3,
      );
    }

    const start = previous?.cover ?? (back ? (detail?.cover ?? detail?.surface) : source.cover);
    const end = back ? source.cover : (detail?.cover ?? detail?.surface);
    const fromArtworks = previous?.artworks ?? (back ? detail?.artworks : source.artworks) ?? [];
    const destination = back ? source : detail;
    const toArtworks = destination?.artworks.length
      ? destination.artworks
      : fromArtworks.map((artwork) => ({
          ...artwork,
          shape:
            artwork.shape && end ? { rect: end, radius: destination?.coverRadius ?? 0 } : undefined,
          rect: end
            ? imagePixelRect(
                end,
                artwork.size,
                destination?.fit ?? "cover",
                destination?.position ?? "50% 50%",
              )
            : artwork.rect,
        }));
    if (start && end && fromArtworks.length) {
      const targetRadius = detail?.coverRadius ?? detail?.radius ?? 0;
      const fromShape = {
        rect: start,
        radius: previous?.coverRadius ?? (back ? targetRadius : source.coverRadius),
      };
      const toShape = { rect: end, radius: back ? source.coverRadius : targetRadius };
      const imageOwned = source.coverMode === "image" && detail?.coverMode === "image";
      const bounds = {
        x: Math.min(start.x, end.x),
        y: Math.min(start.y, end.y),
        width: Math.max(start.x + start.width, end.x + end.width) - Math.min(start.x, end.x),
        height: Math.max(start.y + start.height, end.y + end.height) - Math.min(start.y, end.y),
      };
      coverProxy = proxy("navigation-cover-proxy", bounds);
      coverProxy.style.isolation = "isolate";
      for (const src of new Set([...fromArtworks, ...toArtworks].map((artwork) => artwork.src))) {
        const from = fromArtworks.find((artwork) => artwork.src === src);
        const to = toArtworks.find((artwork) => artwork.src === src);
        const artwork = (from ?? to)!;
        const fromImage = from?.rect ?? imagePixelRect(start, artwork.size, "cover", "50% 50%");
        const toImage =
          to?.rect ??
          imagePixelRect(
            end,
            artwork.size,
            destination?.fit ?? "cover",
            destination?.position ?? "50% 50%",
          );
        const picture = document.createElement("img");
        const imageStart = from?.shape ?? fromShape;
        const imageEnd = to?.shape ?? toShape;
        const box = imageOwned ? imageEnd.rect : toImage;
        picture.src = src;
        picture.alt = "";
        if (imageOwned) picture.dataset.navigationSharedImage = "";
        Object.assign(picture.style, {
          position: "absolute",
          left: "0",
          top: "0",
          display: "block",
          maxWidth: "none",
          width: `${box.width}px`,
          height: `${box.height}px`,
          transformOrigin: "0 0",
          mixBlendMode: "plus-lighter",
          ...(imageOwned
            ? { objectFit: "cover", objectPosition: destination?.position ?? "50% 50%" }
            : {}),
        });
        coverProxy.append(picture);
        artworkProxies.push({ element: picture, artwork });
        const imageTransform = (pixels: SurfaceRect) =>
          `translate3d(${pixels.x - bounds.x}px, ${pixels.y - bounds.y}px, 0) scale(${pixels.width / toImage.width}, ${pixels.height / toImage.height})`;
        // Each photo keeps its own aspect ratio while the shared crop moves.
        animate(
          picture,
          {
            ...(imageOwned
              ? sharedImageFrames(imageStart, imageEnd, bounds)
              : { transform: [imageTransform(fromImage), imageTransform(toImage)] }),
            opacity: [from?.opacity ?? 0, to?.opacity ?? 0],
          },
          layerMotion.shared,
          0,
          true,
        );
      }
      if (!imageOwned) {
        coverProxy.style.willChange = "clip-path";
        const coverFrames = projectedCoverFrames(fromShape, toShape);
        coverRadii = coverFrames.map(({ radius }) => `${radius}px`);
        animate(
          coverProxy,
          { clipPath: coverFrames.map(({ rect, radius }) => cropClip(rect, bounds, radius)) },
          layerMotion.shared,
          0,
          true,
        );
      }
      foreground.dataset.navigationCoverHidden = "";
      cleanups.push(() => {
        delete foreground.dataset.navigationCoverHidden;
      });
    }

    const detailLayout = {
      surface: viewport,
      cover: detail?.cover ?? detail?.surface,
      title: detailTitle,
    };
    for (const decoration of source.decorations) {
      const layout = decorationRect(decoration, source, detailLayout);
      const before = previous?.decorations?.find((value) => value.key === decoration.key);
      const from = before?.rect ?? (back ? layout : decoration.rect);
      const to = back ? decoration.rect : layout;
      const container = decoration.placement.startsWith("cover") ? coverProxy : undefined;
      const origin = container ? rectOf(container) : hostRect;
      const element = proxy(
        "navigation-decoration-proxy",
        from,
        decoration.placement === "surface" || (container && decoration.placement === "cover")
          ? "3"
          : container
            ? "1"
            : "4",
        container,
      );
      applyDecorationPaint(element, decoration);
      decorationProxies.push({ element, key: decoration.key });
      const transform = (rect: SurfaceRect) =>
        `translate3d(${rect.x - origin.x}px, ${rect.y - origin.y}px, 0)`;
      const isFrame = decoration.placement === "surface" || decoration.placement === "cover";
      const sourceRadius = decoration.placement === "surface" ? source.radius : source.coverRadius;
      const detailRadius = decoration.placement === "surface" ? 0 : (detail?.coverRadius ?? 0);
      animate(
        element,
        {
          transform: [transform(from), transform(to)],
          // Only empty paint boxes resize; text retains its measured layout.
          ...(decoration.text
            ? {}
            : { width: [from.width, to.width], height: [from.height, to.height] }),
          ...(isFrame
            ? {
                borderRadius:
                  decoration.placement === "cover" && coverRadii
                    ? coverRadii
                    : [
                        before?.radius ?? `${back ? detailRadius : sourceRadius}px`,
                        `${back ? sourceRadius : detailRadius}px`,
                      ],
              }
            : {}),
        },
        layerMotion.shared,
        0,
        true,
      );
      animate(
        element,
        {
          opacity: [
            before?.opacity ?? (back ? 0 : decoration.opacity),
            back ? decoration.opacity : 0,
          ],
        },
        back ? layerMotion.cardDetails.show : layerMotion.cardDetails.hide,
        back && !before?.opacity ? layerMotion.cardDetails.delay : 0,
      );
    }

    const fromTitle = back ? (previous?.titles?.[0]?.geometry ?? detailTitle) : source.title;
    const toTitle = back ? source.title : detailTitle;
    if (fromTitle && toTitle && fromTitle.text === toTitle.text) {
      const titles = previous?.titles?.filter((title) => title.opacity > 0) ?? [
        { geometry: fromTitle, opacity: 1 },
      ];
      if (!titles.some((title) => sameTitleLayout(title.geometry, toTitle))) {
        titles.push({
          geometry: {
            ...toTitle,
            rect: { ...toTitle.rect, x: fromTitle.rect.x, y: fromTitle.rect.y },
          },
          opacity: 0,
        });
      }
      for (const { geometry: title, opacity } of titles) {
        const target = sameTitleLayout(title, toTitle);
        const element = proxy("navigation-title-proxy", title.rect);
        titleProxies.push(element);
        element.dataset.navigationTitle = "";
        element.textContent = title.text;
        Object.assign(element.style, {
          fontFamily: title.fontFamily,
          fontSize: title.fontSize,
          fontWeight: title.fontWeight,
          lineHeight: title.lineHeight,
          letterSpacing: title.letterSpacing,
          color: title.color,
          whiteSpace: title.whiteSpace,
          textAlign: title.textAlign,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: title.lineClamp,
          overflow: "hidden",
          overflowWrap: "anywhere",
          willChange: "transform, opacity",
        });
        // Keep both endpoint layouts fixed; changing type size rewraps long titles mid-flight.
        moveTitle(element, title.rect, toTitle.rect);
        animate(
          element,
          { opacity: [opacity, target ? 1 : 0] },
          layerMotion.shared * (target ? 0.45 : 0.3),
          target && !opacity ? layerMotion.shared * 0.18 : 0,
        );
      }
      foreground.dataset.navigationTitleHidden = "";
      cleanups.push(() => {
        delete foreground.dataset.navigationTitleHidden;
      });
    }

    const original = live?.element;
    if (original) {
      save(original, "opacity", "pointer-events", "transition");
      const inert = original.inert;
      original.style.transition = "none";
      original.style.opacity = "0";
      original.style.pointerEvents = "none";
      original.inert = true;
      cleanups.push(() => {
        original.inert = inert;
      });
      cleanups.push(observeSource(original, live.geometry, cleanup));
    }
  } else if (transition.direction === "root" || transition.direction === "replace") {
    animate(incoming, { opacity: [0, 1] }, layerMotion.root);
    if (outgoing) animate(outgoing, { opacity: [1, 0] }, layerMotion.root);
  } else {
    prepareSurface();
    const atRest = "translate3d(0px, 0px, 0px)";
    const offscreen = `translate3d(${viewport.width}px, 0px, 0px)`;
    animate(
      foreground,
      { transform: back ? [previous?.transform ?? atRest, offscreen] : [offscreen, atRest] },
      layerMotion.page,
    );
  }
  void Promise.all(animations.map((animation) => Promise.resolve(animation))).then(cleanup);
  return {
    cancel: cleanup,
    snapshot: (): InterruptedSurface | undefined => {
      if (finished) return;
      const style = getComputedStyle(foreground);
      const matrix = new DOMMatrixReadOnly(style.transform);
      const radii = (style.clipPath.split("round ")[1] ?? "0").split("/").map(Number.parseFloat);
      const artworks = artworkProxies.map(({ element, artwork }) => {
        const style = getComputedStyle(element);
        return {
          ...artwork,
          rect: imagePixelRect(
            rectOf(element),
            artwork.size,
            style.objectFit,
            style.objectPosition,
          ),
          opacity: Number(style.opacity),
          shape: element.hasAttribute("data-navigation-shared-image")
            ? imageShapeOf(element)
            : undefined,
        };
      });
      const imageShape = artworks.find((artwork) => artwork.shape)?.shape;
      return {
        element: foreground,
        transform: style.transform,
        scale: {
          x: matrix.a,
          y: matrix.d,
          radiusX: radii[0] * matrix.a,
          radiusY: (radii[1] ?? radii[0]) * matrix.d,
        },
        contentOpacity: content ? getComputedStyle(content).opacity : "1",
        backdropOpacity: backdrop ? Number(getComputedStyle(backdrop).opacity) : undefined,
        elevationOpacity: elevation ? Number(getComputedStyle(elevation).opacity) : undefined,
        cover: imageShape?.rect ?? (coverProxy ? paintedCrop(coverProxy) : undefined),
        coverRadius:
          imageShape?.radius ??
          (coverProxy
            ? Number.parseFloat(getComputedStyle(coverProxy).clipPath.split("round ")[1]) || 0
            : undefined),
        artworks,
        titles: titleProxies.flatMap((element) => {
          const geometry = measureTitle(element);
          return geometry ? [{ geometry, opacity: Number(getComputedStyle(element).opacity) }] : [];
        }),
        decorations: decorationProxies.map(({ element, key }) => ({
          key,
          rect: rectOf(element),
          opacity: Number(getComputedStyle(element).opacity),
          radius: getComputedStyle(element).borderRadius,
        })),
      };
    },
  };
}
