export function normalizeCapabilityIdList(raw: unknown): string[] {
  const items = Array.isArray(raw) ? raw : [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const item of items) {
    const value = String(item || "").trim();
    if (!value) continue;
    if (seen.has(value)) continue;
    seen.add(value);
    out.push(value);
  }
  return out;
}

export function parseCapabilityIdInput(text: string): string[] {
  const chunks = String(text || "")
    .split(/[\n,;]/g)
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
  return normalizeCapabilityIdList(chunks);
}

export function formatCapabilityIdInput(raw: unknown): string {
  return normalizeCapabilityIdList(raw).join("\n");
}
