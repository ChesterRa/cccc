import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vite-plus/test";

import { SUPPORTED_RUNTIMES } from "../../src/types";
import { getRuntimeLogoSrc, RUNTIME_LOGO_FILE_BY_RUNTIME } from "../../src/utils/runtimeLogos";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const WEB_ROOT = resolve(TEST_DIR, "../..");
const PUBLIC_ROOT = resolve(WEB_ROOT, "public");

describe("runtimeLogos", () => {
  it("covers every built-in runtime with a file except the inline Claude logo", () => {
    const expected = SUPPORTED_RUNTIMES.filter(
      (runtime) => runtime !== "custom" && runtime !== "claude",
    ).sort();
    const actual = Object.keys(RUNTIME_LOGO_FILE_BY_RUNTIME).sort();
    expect(actual).toEqual(expected);
  });

  it("maps built-in runtimes to local logo assets that exist", () => {
    for (const runtime of Object.keys(RUNTIME_LOGO_FILE_BY_RUNTIME)) {
      const relativePath =
        RUNTIME_LOGO_FILE_BY_RUNTIME[runtime as keyof typeof RUNTIME_LOGO_FILE_BY_RUNTIME];
      expect(getRuntimeLogoSrc(runtime)).toBe(`${import.meta.env.BASE_URL}${relativePath}`);
      expect(existsSync(resolve(PUBLIC_ROOT, relativePath))).toBe(true);
    }
  });

  it("returns null for the inline Claude logo and custom or unknown runtimes", () => {
    expect(getRuntimeLogoSrc("claude")).toBeNull();
    expect(getRuntimeLogoSrc("custom")).toBeNull();
    expect(getRuntimeLogoSrc("unknown-runtime")).toBeNull();
    expect(getRuntimeLogoSrc("")).toBeNull();
  });
});
