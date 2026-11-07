import { withAuthToken } from "../services/api/base";
import { fileKindForName } from "./messageAttachments";

type PreviewKind = NonNullable<ReturnType<typeof fileKindForName>>;

export type MessageLinkPreview = {
  kind: PreviewKind;
  href: string;
  label: string;
  /** Only this site's files: a download link to another site would replace CCCC. */
  downloadHref?: string;
};

// Text from another site can only be read where that site allows it, which most do not. These
// two do, and a failed preview would cost one more click than the plain link does.
const READABLE_TEXT_HOSTS = new Set(["raw.githubusercontent.com", "gist.githubusercontent.com"]);

// On this site only the Group's files open in the page, never another API answer.
const GROUP_FILE_PATH = /^\/api\/v1\/groups\/[^/]+\/(?:blobs\/[^/]+|workspace\/content)$/;

function fileName(path: string) {
  const name = path.split("/").pop() || "";
  try {
    return decodeURIComponent(name);
  } catch {
    return name;
  }
}

/** A Group file's address carries its name in a parameter; a blob's path is a content hash. */
function groupFileName(url: URL) {
  return url.searchParams.get("filename") || fileName(url.searchParams.get("path") || "");
}

/**
 * What a link in a message opens in the page, or null when it stays an ordinary link. Only the
 * address decides, so nothing is requested until the reader clicks. A PDF on another site stays
 * a link: the browser's own viewer shows it in a new tab, and a frame would run whatever the
 * site answers with.
 */
export function messageLinkPreview(
  href: string,
  origin = window.location.origin,
): MessageLinkPreview | null {
  let url: URL;
  try {
    url = new URL(href, origin);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  const sameSite = url.origin === origin;
  if (sameSite && !GROUP_FILE_PATH.test(url.pathname)) return null;
  const label = sameSite ? groupFileName(url) : fileName(url.pathname);
  const kind = fileKindForName(label);
  if (!kind) return null;
  if (sameSite) {
    // A copied download link is sent as an attachment, which neither a frame nor a player shows.
    const preview = new URL(url);
    preview.searchParams.delete("download");
    const download = new URL(url);
    download.searchParams.set("download", "true");
    return {
      kind,
      href: withAuthToken(preview.href),
      label,
      downloadHref: withAuthToken(download.href),
    };
  }
  if (kind === "image" || kind === "video" || kind === "audio")
    return { kind, href: url.href, label };
  if (kind !== "pdf" && READABLE_TEXT_HOSTS.has(url.hostname))
    return { kind, href: url.href, label };
  return null;
}
