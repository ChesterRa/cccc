import { createHash } from "node:crypto";

export const ENTRY_KEYS = [
  "baseBlobOid",
  "baseLines",
  "formattedLines",
  "formattedSha256",
  "path",
];

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

export function countPythonSplitlines(bytes) {
  const text = bytes.toString("utf8");
  if (!text) return 0;
  const separators = /\r\n|[\n\v\f\r\x1c-\x1e\x85\u2028\u2029]/g;
  const matches = text.match(separators);
  let lines = (matches?.length ?? 0) + 1;
  if (/(?:\r\n|[\n\v\f\r\x1c-\x1e\x85\u2028\u2029])$/.test(text)) lines -= 1;
  return lines;
}

export function validateManifestShape(manifest, formatterVersion) {
  if (
    manifest.version !== 1 ||
    JSON.stringify(manifest.formatter) !==
      JSON.stringify({ name: "oxfmt", version: formatterVersion }) ||
    !Array.isArray(manifest.files) ||
    manifest.files.length === 0
  ) {
    throw new Error("invalid v1 manifest metadata");
  }
  const paths = new Set();
  for (const entry of manifest.files) {
    if (JSON.stringify(Object.keys(entry).sort()) !== JSON.stringify(ENTRY_KEYS)) {
      throw new Error(`invalid fields for ${entry.path ?? "unknown path"}`);
    }
    if (paths.has(entry.path)) throw new Error(`duplicate path ${entry.path}`);
    paths.add(entry.path);
  }
  const sorted = [...paths].sort();
  if (JSON.stringify([...paths]) !== JSON.stringify(sorted)) {
    throw new Error("manifest paths must be sorted");
  }
}

export function verifyImmutableManifest(currentBytes, baseBytes) {
  if (!currentBytes.equals(baseBytes)) {
    throw new Error("v1 manifest must remain byte-for-byte unchanged");
  }
}

export function verifyInitialMigration({
  manifest,
  formatterVersion,
  expectedEntries,
  currentBaseline,
  baseBaseline,
  currentFiles,
  limit,
}) {
  validateManifestShape(manifest, formatterVersion);
  if (JSON.stringify(manifest.files) !== JSON.stringify(expectedEntries)) {
    throw new Error(
      "manifest is not the complete deterministic formatter-only migration for the trusted base",
    );
  }
  for (const entry of manifest.files) {
    const currentBytes = currentFiles.get(entry.path);
    if (!currentBytes || sha256(currentBytes) !== entry.formattedSha256) {
      throw new Error(`${entry.path} current hash changed`);
    }
    if (countPythonSplitlines(currentBytes) !== entry.formattedLines) {
      throw new Error(`${entry.path} current lines changed`);
    }
    if (currentBaseline[entry.path] !== entry.formattedLines) {
      throw new Error(`${entry.path} baseline is not exact`);
    }
    const oldLimit = baseBaseline[entry.path];
    if (oldLimit !== undefined && oldLimit !== entry.baseLines) {
      throw new Error(`${entry.path} old baseline is not exact`);
    }
    if (oldLimit === undefined && !(entry.baseLines <= limit && entry.formattedLines > limit)) {
      throw new Error(`${entry.path} is not a pure <=${limit} to >${limit} crossing`);
    }
  }
}
