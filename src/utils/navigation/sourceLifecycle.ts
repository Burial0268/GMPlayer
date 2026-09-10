import { rectOf, type SourceGeometry } from "./sources";

/** A captured visual stops being valid when its live destination changes. */
export function observeSource(
  element: HTMLElement,
  geometry: SourceGeometry,
  invalidate: () => void,
): () => void {
  const identity = element.dataset.navigationIdentity;
  const source = element.dataset.navigationSource;
  const changed = () => {
    if (
      !element.isConnected ||
      element.dataset.navigationIdentity !== identity ||
      element.dataset.navigationSource !== source
    )
      return true;
    const rect = rectOf(element);
    return (["x", "y", "width", "height"] as const).some(
      (key) => Math.abs(rect[key] - geometry.surface[key]) > 0.5,
    );
  };
  const resize = new ResizeObserver(() => {
    if (changed()) invalidate();
  });
  const mutation = new MutationObserver((records) => {
    const contentChanged = records.some(
      (record) =>
        element.contains(record.target) &&
        (record.type !== "attributes" || record.attributeName !== "style"),
    );
    if (contentChanged || changed()) invalidate();
  });
  resize.observe(element);
  mutation.observe(element.parentElement ?? element, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: [
      "style",
      "class",
      "src",
      "srcset",
      "data-navigation-identity",
      "data-navigation-source",
    ],
  });
  return () => {
    resize.disconnect();
    mutation.disconnect();
  };
}
