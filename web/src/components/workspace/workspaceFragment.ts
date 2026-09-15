/** Scroll only the document viewport; leave the surrounding workspace controls in place. */
export function scrollWorkspaceFragment(root: Document | HTMLElement, fragment: string) {
  let id: string;
  try {
    id = decodeURIComponent(fragment.replace(/^#/, ""));
  } catch {
    return;
  }
  const isDocument = root.nodeType === 9;
  const scroller = isDocument ? (root as Document).scrollingElement : (root as HTMLElement);
  if (!scroller) return;
  if (!id) {
    scroller.scrollTop = 0;
    return;
  }
  const escaped = CSS.escape(id);
  const target = root.querySelector(`#${escaped}, a[name="${escaped}"]`);
  if (!target) return;
  scroller.scrollTop +=
    target.getBoundingClientRect().top -
    (isDocument ? 0 : scroller.getBoundingClientRect().top + (scroller as HTMLElement).clientTop);
}
