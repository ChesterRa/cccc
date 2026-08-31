export const MAX_INLINE_COMPOSER_TEXT_BYTES = 64 * 1024;

export type PreparedComposerMessage = { text: string; files: File[]; converted: boolean };

function messageAttachmentName(now: number): string {
  const timestamp = new Date(now).toISOString().replace(/\D/g, "").slice(0, 14);
  return `cccc-message-${timestamp}.txt`;
}

export function prepareComposerMessage(input: {
  text: string;
  files: File[];
  now?: number;
  maxInlineBytes?: number;
  targets?: Array<{ isCrossGroup?: boolean; isRemote?: boolean }>;
}): PreparedComposerMessage {
  const text = String(input.text || "").trim();
  const files = input.files.slice();
  const maxInlineBytes = input.maxInlineBytes ?? MAX_INLINE_COMPOSER_TEXT_BYTES;
  if (
    input.targets?.some((target) => target.isCrossGroup && !target.isRemote) ||
    new TextEncoder().encode(text).byteLength <= maxInlineBytes
  ) {
    return { text, files, converted: false };
  }

  const now = input.now ?? Date.now();
  const name = messageAttachmentName(now);
  const attachment = new File([text], name, {
    type: "text/plain;charset=utf-8",
    lastModified: now,
  });
  return { text: `[file] ${name}`, files: [...files, attachment], converted: true };
}
