/** The most text a preview reads, as in the Files panel. */
export const DOCUMENT_PREVIEW_BYTES = 1024 * 1024;

const BINARY_CHECK_BYTES = 8192;

const PDF_SIGNATURE = [0x25, 0x50, 0x44, 0x46, 0x2d];

export type DocumentText =
  | { status: "ready"; text: string; truncated: boolean }
  | { status: "binary" };

/** Stops reading at the limit: Content-Length may be missing or wrong. */
async function readLimited(response: Response, limit: number) {
  const reader = response.body?.getReader();
  if (!reader) return new Uint8Array(await response.arrayBuffer());
  const chunks: Uint8Array[] = [];
  let length = 0;
  while (length <= limit) {
    const { done, value } = await reader.read();
    if (done) break;
    // A single read can hand over far more than the limit.
    const kept = value.subarray(0, limit + 1 - length);
    chunks.push(kept);
    length += kept.length;
  }
  await reader.cancel();
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes;
}

async function request(href: string, signal: AbortSignal) {
  const response = await fetch(href, { credentials: "same-origin", signal });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response;
}

/**
 * A frame starts a download for anything the browser does not take as an inline PDF, and a file
 * name proves nothing, so the response decides before the frame exists. A GET, because a local
 * blob: address does not answer HEAD.
 */
export async function isPdfDocument(href: string, signal: AbortSignal) {
  const response = await request(href, signal);
  const type = response.headers.get("content-type")?.split(";")[0].trim().toLowerCase();
  const attachment = /^\s*attachment/i.test(response.headers.get("content-disposition") || "");
  const bytes = await readLimited(response, PDF_SIGNATURE.length);
  return (
    type === "application/pdf" &&
    !attachment &&
    PDF_SIGNATURE.every((byte, index) => bytes[index] === byte)
  );
}

export async function readDocumentText(href: string, signal: AbortSignal): Promise<DocumentText> {
  const bytes = await readLimited(await request(href, signal), DOCUMENT_PREVIEW_BYTES);
  if (bytes.subarray(0, BINARY_CHECK_BYTES).includes(0)) return { status: "binary" };
  const truncated = bytes.length > DOCUMENT_PREVIEW_BYTES;
  // A streaming decode leaves out a character the limit cut in half.
  const text = new TextDecoder().decode(bytes.subarray(0, DOCUMENT_PREVIEW_BYTES), {
    stream: truncated,
  });
  return { status: "ready", text, truncated };
}
