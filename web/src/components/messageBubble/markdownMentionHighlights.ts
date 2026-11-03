import { createHighlighterAnnotation } from "../ui/highlighterAnnotation";
import { findMessageMentionRanges, messageMentionColor } from "../../utils/mentionHighlight";

export function highlightMarkdownMentions(
  root: HTMLElement,
  knownTokens: string[],
  animate: boolean,
) {
  const document = root.ownerDocument;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    if (
      node.parentElement?.closest(
        "a, button, code, pre, script, style, svg, [data-message-mention]",
      )
    )
      continue;
    nodes.push(node);
  }

  const spans: HTMLSpanElement[] = [];
  for (const node of nodes) {
    const text = node.data;
    const ranges = findMessageMentionRanges(text, knownTokens);
    if (!ranges.length) continue;
    const fragment = document.createDocumentFragment();
    let cursor = 0;
    for (const range of ranges) {
      fragment.append(document.createTextNode(text.slice(cursor, range.start)));
      const span = document.createElement("span");
      span.className = "relative";
      span.dataset.messageMention = range.kind;
      span.textContent = text.slice(range.start, range.end);
      fragment.append(span);
      spans.push(span);
      cursor = range.end;
    }
    fragment.append(document.createTextNode(text.slice(cursor)));
    node.replaceWith(fragment);
  }

  const annotations = spans.map((span) =>
    createHighlighterAnnotation(span, {
      type: "highlight",
      color: messageMentionColor(span.dataset.messageMention === "group" ? "group" : "agent"),
      animationDuration: 300,
      iterations: 1,
      padding: 0,
      multiline: true,
      animate,
    }),
  );
  const observer =
    annotations.length && typeof ResizeObserver !== "undefined"
      ? new ResizeObserver(() => {
          for (const annotation of annotations) {
            annotation.hide();
            annotation.show();
          }
        })
      : null;
  observer?.observe(root);

  return () => {
    observer?.disconnect();
    for (const annotation of annotations) annotation.remove();
    for (const span of spans) {
      const parent = span.parentNode;
      if (!parent) continue;
      span.replaceWith(document.createTextNode(span.textContent || ""));
      parent.normalize();
    }
  };
}
