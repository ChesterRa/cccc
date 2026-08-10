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
  if (localPreviewUrl.startsWith("blob:")) {
    return { label, downloadName, previewHref: localPreviewUrl, downloadHref: localPreviewUrl };
  }

  const base = `/api/v1/groups/${encodeURIComponent(input.groupId)}/blobs/${encodeURIComponent(blobName || "")}`;
  const previewParams = new URLSearchParams({ filename: downloadName });
  const downloadParams = new URLSearchParams(previewParams);
  downloadParams.set("download", "true");
  return {
    label,
    downloadName,
    previewHref: withAuthToken(`${base}?${previewParams.toString()}`),
    downloadHref: withAuthToken(`${base}?${downloadParams.toString()}`),
  };
}
