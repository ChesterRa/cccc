import type { SupportedRuntime } from "../types";

type RuntimeLogoRuntime = Exclude<SupportedRuntime, "custom">;

export const RUNTIME_LOGO_FILE_BY_RUNTIME: Partial<Record<RuntimeLogoRuntime, string>> = {
  amp: "logos/amp.png",
  auggie: "logos/auggie.png",
  claude: "logos/claude.png",
  codex: "logos/codex.png",
  droid: "logos/droid.png",
  grok: "logos/grok.svg",
  hermes: "logos/hermes.svg",
  kimi: "logos/kimi.png",
  opencode: "logos/opencode.svg",
  web_model: "logos/codex.png",
};

function normalizeRuntime(runtime: string | null | undefined): RuntimeLogoRuntime {
  return String(runtime || "").trim().toLowerCase() as RuntimeLogoRuntime;
}

export function getRuntimeLogoSrc(runtime: string | null | undefined): string | null {
  const relativePath = RUNTIME_LOGO_FILE_BY_RUNTIME[normalizeRuntime(runtime)];
  return relativePath ? `${import.meta.env.BASE_URL}${relativePath}` : null;
}
