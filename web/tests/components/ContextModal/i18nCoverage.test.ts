import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vite-plus/test";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../../..");

function collectContextKeys(): string[] {
  const pattern = /tr\(\s*"([^"]+)"/g;
  const seen = new Set<string>();
  const queue = [
    resolve(repoRoot, "web/src/components/ContextModal"),
    resolve(repoRoot, "web/src/features/contextModal"),
  ];

  while (queue.length > 0) {
    const current = queue.pop();
    if (!current) continue;
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const next = resolve(current, entry.name);
      if (entry.isDirectory()) {
        queue.push(next);
        continue;
      }
      if (!entry.isFile() || (!next.endsWith(".ts") && !next.endsWith(".tsx"))) {
        continue;
      }
      const text = readFileSync(next, "utf8");
      for (const match of text.matchAll(pattern)) {
        const key = String(match[1] || "");
        if (key.startsWith("context.")) {
          seen.add(key);
        }
      }
    }
  }

  return [...seen].sort();
}

function readLocale(locale: "en" | "ja" | "zh"): Record<string, unknown> {
  return JSON.parse(
    readFileSync(resolve(repoRoot, `web/src/i18n/locales/${locale}/modals.json`), "utf8"),
  ) as Record<string, unknown>;
}

function lookupKey(tree: Record<string, unknown>, dottedKey: string): boolean {
  let node: unknown = tree;
  for (const part of dottedKey.split(".")) {
    if (!node || typeof node !== "object" || !(part in node)) {
      return false;
    }
    node = (node as Record<string, unknown>)[part];
  }
  return true;
}

describe("ContextModal i18n coverage", () => {
  it("keeps all context.* translation keys present across locales", () => {
    const keys = collectContextKeys();
    for (const locale of ["en", "ja", "zh"] as const) {
      const tree = readLocale(locale);
      const missing = keys.filter((key) => !lookupKey(tree, key));
      expect(missing, `${locale} missing: ${missing.join(", ")}`).toEqual([]);
    }
  });
});
