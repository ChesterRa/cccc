/** Scripts stay disabled even though the parent can handle document-link clicks. */
export function staticWorkspaceHtml(
  content: string,
  resolveUrl: (url: string, kind: "image" | "link") => string,
  resourceEndpoint: string,
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
      if (node.hasAttribute(attribute))
        node.setAttribute(attribute, resolveUrl(node.getAttribute(attribute) || "", "image"));
    }
  }
  for (const node of doc.querySelectorAll("audio, video")) {
    node.setAttribute("controls", "");
    node.setAttribute("preload", "metadata");
  }
  for (const node of doc.querySelectorAll("a[href], area[href]")) {
    node.setAttribute("href", resolveUrl(node.getAttribute("href") || "", "link"));
    node.removeAttribute("target");
    node.removeAttribute("download");
  }
  for (const node of doc.querySelectorAll("input, button, select, textarea, fieldset")) {
    node.setAttribute("disabled", "");
  }
  const csp = document.createElement("meta");
  csp.httpEquiv = "Content-Security-Policy";
  csp.content = `default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src ${resourceEndpoint} data:; media-src ${resourceEndpoint}; font-src data:; form-action 'none'; base-uri 'none'`;
  let head = doc.querySelector("head");
  if (!head) {
    head = doc.ownerDocument.createElement("head");
    doc.prepend(head);
  }
  head.prepend(csp);
  return "<!doctype html>" + doc.outerHTML;
}
