import { useMemo } from "react";
import type { Actor } from "../types";

/**
 * Creates a lookup map from actor ID to display name (title or id).
 * Memoized to avoid recreating on every render.
 */
export function useActorDisplayNameMap(actors: Actor[]): Map<string, string> {
  return useMemo(() => {
    const map = new Map<string, string>();
    for (const actor of actors) {
      const id = String(actor.id || "");
      if (id) {
        map.set(id, actor.title || id);
      }
    }
    return map;
  }, [actors]);
}

/**
 * Get display name for a recipient token.
 * Special tokens (@all, @foreman, @peers, user) are returned as-is.
 */
export function getRecipientDisplayName(
  token: string,
  displayNameMap: Map<string, string>,
): string {
  if (token.startsWith("@") || token === "user") {
    return token;
  }
  return displayNameMap.get(token) || token;
}
