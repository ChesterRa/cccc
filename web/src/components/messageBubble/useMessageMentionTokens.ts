import { useMemo } from "react";
import type { Actor } from "../../types";
import { getMessageMentionTokens, type MessageMentionToken } from "../../utils/mentionHighlight";

/**
 * The references a message can highlight. The actor list is replaced on every activity or unread
 * update, so the result keeps its identity until its content changes; rendered messages are then
 * not parsed again.
 */
export function useMessageMentionTokens(
  actors: Actor[],
  data: unknown,
): readonly MessageMentionToken[] {
  const key = JSON.stringify(getMessageMentionTokens(actors, data));
  return useMemo(() => JSON.parse(key) as MessageMentionToken[], [key]);
}
