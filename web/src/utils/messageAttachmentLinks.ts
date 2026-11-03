import { withAuthToken } from "../services/api/base";

type AttachmentLinkInput = {
  groupId: string;
  path?: string;
  title?: string;
  localPreviewUrl?: string;
  fallbackLabel: string;
};

export type MessageAttachmentLinks = {
  label: string;
  downloadName: string;
  previewHref: string;
  downloadHref: string;
};

export function safeAttachmentFilename(value: string, fallback = "download"): string {
  const leaf = String(value || "")
    .split(/[\\/]/)
    .pop()
    ?.trim();
  const cleaned = Array.from(leaf || "")
    .filter((character) => {
      const code = character.charCodeAt(0);
      return code > 0x1f && code !== 0x7f;
    })
    .map((character) => (character === '"' ? "_" : character))
    .join("")
    .slice(0, 180);
  return cleaned && cleaned !== "." && cleaned !== ".." ? cleaned : fallback;
}

export function buildMessageAttachmentLinks(input: AttachmentLinkInput): MessageAttachmentLinks {
  const blobName = String(input.path || "")
    .split("/")
    .pop()
    ?.trim();
  const label = String(input.title || "").trim() || blobName || input.fallbackLabel;
  const downloadName = safeAttachmentFilename(label, input.fallbackLabel);
  const localPreviewUrl = String(input.localPreviewUrl || "").trim();
  const local = localPreviewUrl.startsWith("blob:") ? localPreviewUrl : "";
  // Until the upload is stored, the local copy is the only one.
  if (local && !blobName) {
    return { label, downloadName, previewHref: local, downloadHref: local };
  }

  const base = `/api/v1/groups/${encodeURIComponent(input.groupId)}/blobs/${encodeURIComponent(blobName || "")}`;
  const previewParams = new URLSearchParams({ filename: downloadName });
  const downloadParams = new URLSearchParams(previewParams);
  downloadParams.set("download", "true");
  // Once stored, everything loads the stored blob: the local URL is revoked shortly after the
  // upload is confirmed, and a message can be shown again long after that, for example after
  // a filter change. A raster image on screen keeps showing until its stored copy has loaded
  // (ImagePreview), so the switch is not seen; an SVG shows its placeholder while it is fetched.
  return {
    label,
    downloadName,
    previewHref: withAuthToken(`${base}?${previewParams.toString()}`),
    downloadHref: withAuthToken(`${base}?${downloadParams.toString()}`),
  };
}
