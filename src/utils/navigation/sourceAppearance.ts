import type { SourceGeometry, SurfaceRect } from "./sources";

type Placement =
  | "surface"
  | "cover"
  | "cover-bottom"
  | "cover-top-left"
  | "cover-top-right"
  | "cover-bottom-left"
  | "title-bottom";

export interface SourceDecoration {
  key: string;
  placement: Placement;
  rect: SurfaceRect;
  text?: string;
  style: Record<string, string>;
  opacity: number;
}

export interface InterruptedDecoration {
  key: string;
  rect: SurfaceRect;
  opacity: number;
  radius: string;
}

const paintProperties = [
  "background-color",
  "background-image",
  "background-size",
  "background-position",
  "background-repeat",
  "border-top",
  "border-right",
  "border-bottom",
  "border-left",
  "border-radius",
  "padding-top",
  "padding-right",
  "padding-bottom",
  "padding-left",
  "box-shadow",
  "backdrop-filter",
  "mix-blend-mode",
  "color",
  "font-family",
  "font-size",
  "font-weight",
  "font-style",
  "line-height",
  "letter-spacing",
  "text-align",
  "text-transform",
  "text-overflow",
  "white-space",
  "display",
  "align-items",
  "justify-items",
];

/** Only opted-in paint and plain text are copied; no component DOM or CSS animations. */
export function captureDecorations(element: HTMLElement): SourceDecoration[] {
  const nodes = [element, ...element.querySelectorAll<HTMLElement>("[data-navigation-decoration]")];
  return nodes.flatMap((node, index) => {
    const placement = node.dataset.navigationDecoration as Placement | undefined;
    if (!placement) return [];
    const style = getComputedStyle(node);
    const { x, y, width, height } = node.getBoundingClientRect();
    if (style.display === "none" || !width || !height || Number(style.opacity) === 0) return [];
    const label =
      placement.endsWith("left") || placement.endsWith("right") || placement === "title-bottom";
    return [
      {
        key: `${placement}:${index}`,
        placement,
        rect: { x, y, width, height },
        text: label ? node.textContent?.trim() : undefined,
        style: Object.fromEntries(
          paintProperties.map((name) => [name, style.getPropertyValue(name)]),
        ),
        opacity: Number(style.opacity),
      },
    ];
  });
}

export function applyDecorationPaint(element: HTMLElement, decoration: SourceDecoration) {
  for (const [name, value] of Object.entries(decoration.style))
    element.style.setProperty(name, value);
  if (decoration.text) element.textContent = decoration.text;
  element.dataset.placement = decoration.placement;
  Object.assign(element.style, {
    boxSizing: "border-box",
    overflow: "hidden",
    visibility: "visible",
    transition: "none",
    animation: "none",
    willChange: "transform, opacity",
  });
}

type DecorationLayout = Pick<SourceGeometry, "surface" | "cover" | "title">;

export function decorationRect(
  decoration: SourceDecoration,
  source: DecorationLayout,
  target: DecorationLayout,
): SurfaceRect {
  const { placement, rect } = decoration;
  if (placement === "surface") return target.surface;
  if (placement === "cover") return target.cover ?? rect;
  const title = placement === "title-bottom";
  const from = title ? source.title?.rect : source.cover;
  const to = title ? target.title?.rect : target.cover;
  if (!from || !to) return rect;
  const bottom = title || placement.includes("bottom");
  const right = placement.endsWith("right");
  return {
    x: to.x + rect.x - from.x + (right ? to.width - from.width : 0),
    y: to.y + rect.y - from.y + (bottom ? to.height - from.height : 0),
    width: rect.width + (placement === "cover-bottom" ? to.width - from.width : 0),
    height: rect.height,
  };
}
