/** Scripts stay disabled even though the parent can handle document-link clicks. */
export function staticWorkspaceHtml(
  content: string,
  resolveUrl: (url: string, kind: "image" | "link") => string,
  resourceEndpoint: string | null,
) {
  // Template contents are inert: parsing must not fetch an original iframe, image
  // or stylesheet before its URL and policy have been checked.
  const template = document.createElement("template");
  const doc = template.content.ownerDocument.createElement("html");
  doc.innerHTML = content;
  doc
    .querySelectorAll("script, iframe, frame, frameset, object, embed, base, meta, link")
    .forEach((node) => node.remove());
  for (const node of doc.querySelectorAll("*")) {
    for (const attribute of Array.from(node.attributes)) {
      if (
        /^on/i.test(attribute.name) ||
        ["srcset", "srcdoc", "action", "formaction", "autoplay", "ping"].includes(attribute.name)
      )
        node.removeAttribute(attribute.name);
    }
  }
  for (const node of doc.querySelectorAll(
    "img[src], audio[src], video[src], source[src], video[poster]",
  )) {
    for (const attribute of ["src", "poster"]) {
      if (node.hasAttribute(attribute)) {
        const resolved = resolveUrl(node.getAttribute(attribute) || "", "image");
        if (resolved) node.setAttribute(attribute, resolved);
        else node.removeAttribute(attribute);
      }
    }
  }
  for (const node of doc.querySelectorAll("audio, video")) {
    node.setAttribute("controls", "");
    node.setAttribute("preload", "metadata");
  }
  for (const node of doc.querySelectorAll("a[href], area[href]")) {
    const resolved = resolveUrl(node.getAttribute("href") || "", "link");
    if (resolved) node.setAttribute("href", resolved);
    else node.removeAttribute("href");
    node.removeAttribute("target");
    node.removeAttribute("download");
  }
  for (const node of doc.querySelectorAll("input, button, select, textarea, fieldset")) {
    node.setAttribute("disabled", "");
  }
  const csp = document.createElement("meta");
  csp.httpEquiv = "Content-Security-Policy";
  // A standalone attachment has no directory or authenticated sibling resources.
  const images = resourceEndpoint ? `${resourceEndpoint} data:` : "data:";
  csp.content = `default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src ${images}; media-src ${resourceEndpoint ?? "'none'"}; font-src data:; form-action 'none'; base-uri 'none'`;
  let head = doc.querySelector("head");
  if (!head) {
    head = doc.ownerDocument.createElement("head");
    doc.prepend(head);
  }
  head.prepend(csp);
  return "<!doctype html>" + doc.outerHTML;
}

/** Keep oversized, centered column items reachable without scaling the document. */
export function preserveStaticHtmlAlignment(doc: Document) {
  const view = doc.defaultView;
  let frame: Element | null;
  try {
    frame = view?.frameElement ?? null;
  } catch {
    // A saved document's window may already have navigated to another origin.
    return () => {};
  }
  if (!view || frame?.localName !== "iframe") return () => {};
  let changed: { element: HTMLElement; value: string; priority: string }[] = [];
  const restore = () => {
    for (const { element, value, priority } of changed)
      element.style.setProperty("align-items", value, priority);
    changed = [];
  };
  const update = () => {
    restore();
    // A user-followed link may have navigated the frame to another origin.
    if ((frame as HTMLIFrameElement).contentDocument !== doc) return;
    const styles = new Map<Element, CSSStyleDeclaration>();
    const style = (element: Element) => {
      let value = styles.get(element);
      if (!value) {
        value = view.getComputedStyle(element);
        styles.set(element, value);
      }
      return value;
    };
    const centered: HTMLElement[] = [];
    for (const element of doc.querySelectorAll<HTMLElement>("*")) {
      if (!element.childElementCount) continue;
      const css = style(element);
      if (
        !["flex", "inline-flex"].includes(css.display) ||
        !css.flexDirection.startsWith("column") ||
        css.alignItems !== "center" ||
        css.writingMode !== "horizontal-tb" ||
        css.direction !== "ltr"
      )
        continue;
      // An author may deliberately crop an oversized child to a centered window.
      let clipped = false;
      let origin = 0;
      let scroll = view.scrollX;
      let innerScroller = false;
      for (let parent: HTMLElement | null = element; parent; parent = parent.parentElement) {
        const parentCss = style(parent);
        if (["hidden", "clip"].includes(parentCss.overflowX)) {
          clipped = true;
          break;
        }
        if (
          !innerScroller &&
          parent !== doc.body &&
          parent !== doc.documentElement &&
          ["auto", "scroll"].includes(parentCss.overflowX)
        ) {
          if (parentCss.direction !== "ltr") {
            clipped = true;
            break;
          }
          origin = parent.getBoundingClientRect().left + parent.clientLeft;
          scroll = parent.scrollLeft;
          innerScroller = true;
        }
      }
      if (clipped || (!innerScroller && style(doc.documentElement).direction !== "ltr")) continue;
      const start = element.getBoundingClientRect().left;
      if (
        Array.from(element.children).some((child) => {
          const left = child.getBoundingClientRect().left;
          return left < start - 0.5 && left + scroll < origin - 0.5;
        })
      )
        centered.push(element);
    }
    // Read the author's current layout first; writes must not affect subsequent measurements.
    for (const element of centered) {
      changed.push({
        element,
        value: element.style.getPropertyValue("align-items"),
        priority: element.style.getPropertyPriority("align-items"),
      });
      element.style.setProperty("align-items", "safe center", "important");
    }
  };
  update();
  const observer = new ResizeObserver(update);
  observer.observe(frame);
  return () => {
    observer.disconnect();
    restore();
  };
}
