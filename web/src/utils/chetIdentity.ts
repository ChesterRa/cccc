import type { GroupDoc, LedgerEvent } from "../types";

/** How a Group stores Chet: a service identity, not a member. */
export const CHET_PRINCIPAL = "svc:chet";
/** How a Group addresses Chet: alone, with Send or Request reply. */
export const CHET_SELECTOR = "@chet";
export const CHET_NAME = "Chet";
/** The host stamps each message Chet writes with the Group generation it was written in. */
const GENERATION_FIELD = "_cccc_chet_group_generation";

/** The generation Chet's messages carry, or "" until the Group is known. */
export function chetGeneration(group: Pick<GroupDoc, "generation" | "created_at"> | null): string {
  if (!group) return "";
  return group.generation || (group.created_at ? `legacy:${group.created_at}` : "");
}

/**
 * Whether Chet wrote this message in the Group whose generation is given. A copied or
 * re-created Group keeps the event but not that identity, so its old messages show the
 * stored identity instead of Chet's name.
 */
export function isChetMessage(event: LedgerEvent, generation: string): boolean {
  if (!generation || event.kind !== "chat.message" || event.by !== CHET_PRINCIPAL) return false;
  const data = event.data as Record<string, unknown> | undefined;
  return data?.[GENERATION_FIELD] === generation;
}
