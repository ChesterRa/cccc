import type { SupportedRuntime } from "../types";

type RuntimeLogoRuntime = Exclude<SupportedRuntime, "custom">;

export const RUNTIME_LOGO_FILE_BY_RUNTIME: Partial<Record<RuntimeLogoRuntime, string>> = {
  amp: "logos/amp.png",
  auggie: "logos/auggie.png",
  claude: "logos/claude.png",
  codex: "logos/codex.png",
  droid: "logos/droid.png",
  gemini: "logos/gemini.png",
  kimi: "logos/kimi.png",
  neovate: "logos/neovate.png",
};

export function getRuntimeLogoSrc(runtime: string | null | undefined): string | null {
  const normalizedRuntime = String(runtime || "").trim().toLowerCase() as RuntimeLogoRuntime;
  const relativePath = RUNTIME_LOGO_FILE_BY_RUNTIME[normalizedRuntime];
  return relativePath ? `${import.meta.env.BASE_URL}${relativePath}` : null;
}
