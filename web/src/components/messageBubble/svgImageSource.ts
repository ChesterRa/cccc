/** Maximum SVG attachment size decoded into an opaque-origin data URL for `<img>` preview. */
export const MAX_SVG_PREVIEW_BYTES = 4 * 1024 * 1024;

const BASE64_CHUNK_BYTES = 0x8000;

export type ResolvedSvgImageSource =
  | { kind: "data"; src: string; isSvg: true }
  | { kind: "raster"; blob: Blob; isSvg: false };

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_BYTES) {
    const chunk = bytes.subarray(offset, offset + BASE64_CHUNK_BYTES);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

/**
 * Resolves a fetched `.svg` attachment into a safe `<img>` source without
 * creating a same-origin `blob:` SVG document URL (which would execute embedded
 * scripts if opened in a new tab). Raster images renamed `.svg` (detected by
 * server magic bytes) remain raster blobs; oversized blobs fall back to download;
 * all other payloads become opaque-origin `data:image/svg+xml;base64,...` URLs
 * where non-SVG content fails image decoding via the existing `<img>` error path.
 */
export async function resolveSvgImageSource(blob: Blob): Promise<ResolvedSvgImageSource> {
  const mime = blob.type.split(";", 1)[0].trim().toLowerCase();
  if (mime.startsWith("image/") && mime !== "image/svg+xml") {
    return { kind: "raster", blob, isSvg: false };
  }
  if (blob.size > MAX_SVG_PREVIEW_BYTES) {
    throw new Error(`svg_too_large:${blob.size}`);
  }
  const bytes = new Uint8Array(await blob.arrayBuffer());
  if (bytes.length > MAX_SVG_PREVIEW_BYTES) {
    throw new Error(`svg_too_large:${bytes.length}`);
  }
  return { kind: "data", src: `data:image/svg+xml;base64,${bytesToBase64(bytes)}`, isSvg: true };
}
