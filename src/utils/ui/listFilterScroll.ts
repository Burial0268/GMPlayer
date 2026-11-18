/**
 * Keep an in-page filter's results starting right under its sticky toolbar.
 *
 * Call it from a `flush: "pre"` watch on the keyword, so it measures the list before the filtered
 * one renders. It moves the page only when the list is already scrolled under the toolbar, and
 * then instantly: smooth-scrolling to the page top on every keystroke threw the toolbar (and the
 * input the user is typing in) away from where they were looking.
 */
export const alignListUnderToolbar = (toolbar: HTMLElement | null | undefined): void => {
  const list = toolbar?.nextElementSibling;
  const scroller = toolbar?.closest<HTMLElement>(".n-scrollbar-container");
  if (!toolbar || !list || !scroller) return;
  const overlap = toolbar.getBoundingClientRect().bottom - list.getBoundingClientRect().top;
  if (overlap > 0) scroller.scrollBy({ top: -overlap, behavior: "instant" });
};
