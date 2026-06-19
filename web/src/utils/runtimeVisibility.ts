import type { Actor } from "../types";

export type RuntimeVisibilityMode = "hidden" | "visible";

export type RuntimeVisibilityState = {
  peerRuntimeVisibility: RuntimeVisibilityMode;
  assistantRuntimeVisibility: RuntimeVisibilityMode;
};

export const DEFAULT_PEER_RUNTIME_VISIBILITY: RuntimeVisibilityMode = "visible";
export const DEFAULT_ASSISTANT_RUNTIME_VISIBILITY: RuntimeVisibilityMode = "hidden";

export function normalizeRuntimeVisibilityMode(
  value: unknown,
  fallback: RuntimeVisibilityMode,
): RuntimeVisibilityMode {
  const normalized = String(value || "").trim().toLowerCase();
  return normalized === "hidden" || normalized === "visible" ? normalized : fallback;
}

export function isUnsupportedInternalRuntimeActor(actor: Actor | null | undefined): boolean {
  const internalKind = String(actor?.internal_kind || "").trim().toLowerCase();
  return Boolean(internalKind && internalKind !== "voice_secretary");
}

export function isAssistantRuntimeActor(actor: Actor | null | undefined): boolean {
  return String(actor?.internal_kind || "").trim().toLowerCase() === "voice_secretary";
}

export function isRuntimeSurfaceActorVisible(
  actor: Actor | null | undefined,
  options: Partial<RuntimeVisibilityState>,
): boolean {
  if (!actor) return false;
  if (isUnsupportedInternalRuntimeActor(actor)) return false;
  if (isAssistantRuntimeActor(actor)) {
    const assistantRuntimeVisibility = normalizeRuntimeVisibilityMode(
      options.assistantRuntimeVisibility,
      DEFAULT_ASSISTANT_RUNTIME_VISIBILITY,
    );
    return assistantRuntimeVisibility === "visible";
  }
  const peerRuntimeVisibility = normalizeRuntimeVisibilityMode(
    options.peerRuntimeVisibility,
    DEFAULT_PEER_RUNTIME_VISIBILITY,
  );
  return peerRuntimeVisibility === "visible";
}

export function filterVisibleRuntimeActors(
  actors: Actor[],
  options: Partial<RuntimeVisibilityState>,
): Actor[] {
  return actors.filter((actor) => isRuntimeSurfaceActorVisible(actor, options));
}
