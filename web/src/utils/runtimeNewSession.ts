const NEW_SESSION_RUNTIMES = new Set(["claude", "codex", "gemini"]);

export function supportsRuntimeNewSession(runtime: string | null | undefined): boolean {
  return NEW_SESSION_RUNTIMES.has(String(runtime || "").trim().toLowerCase());
}
