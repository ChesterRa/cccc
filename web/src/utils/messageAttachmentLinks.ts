import { withAuthToken } from "../services/api/base";

type AttachmentLinkInput = {
  groupId: string;
  path?: string;
  title?: string;
  localPreviewUrl?: string;
  fallbackLabel: string;
  /** Load on demand: never rely on a local URL that may be revoked by then. */
  preferStoredPreview?: boolean;
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
  // An image already shown from the local copy keeps it to avoid a reload
  // flash. Anything fetched later uses the stored blob: the local URL is
  // revoked shortly after the upload is confirmed.
  return {
    label,
    downloadName,
    previewHref:
      local && !input.preferStoredPreview
        ? local
        : withAuthToken(`${base}?${previewParams.toString()}`),
    downloadHref: withAuthToken(`${base}?${downloadParams.toString()}`),
  };
}
