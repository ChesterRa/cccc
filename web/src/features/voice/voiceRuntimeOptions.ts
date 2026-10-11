import { RUNTIME_INFO, supportsAcpMode } from "../../types";

// Actor Profiles, Voice Analyst and Secretary must offer the same controlled runtimes.
export const structuredVoiceRuntimes = [
  "codex",
  "claude",
  "grok",
  "opencode",
  "kilo",
  "pi",
  "antigravity",
  "copilot",
  "devin",
  "cursor",
  "kimi",
  "auggie",
  "kiro",
  "hermes",
  "droid",
] as const;

export function structuredVoiceRuntimeLabel(runtime: string): string {
  const label = RUNTIME_INFO[runtime]?.label || runtime;
  return supportsAcpMode(runtime) ? `${label} (ACP)` : runtime === "pi" ? `${label} (RPC)` : label;
}
