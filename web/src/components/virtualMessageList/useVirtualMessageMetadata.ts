import { useMemo } from "react";
import { useActorDisplayNameMap } from "../../hooks/useActorDisplayName";
import type { Actor, AgentState, LedgerEvent } from "../../types";
import { buildGroupBridgeDisplayNameMap } from "../virtualMessageListGroupBridge";

export function useVirtualMessageMetadata(
  messages: LedgerEvent[],
  actors: Actor[],
  agentStates: AgentState[],
) {
  const messageTextById = useMemo(() => {
    const values = new Map<string, string>();
    for (const message of messages) {
      const id = String(message?.id || "").trim();
      const data = message?.data as { text?: unknown } | undefined;
      const text = typeof data?.text === "string" ? data.text.trim() : "";
      if (id && text) values.set(id, text);
    }
    return values;
  }, [messages]);

  const agentStateById = useMemo(() => {
    const values = new Map<string, AgentState>();
    for (const state of agentStates || []) values.set(String(state.id || ""), state);
    return values;
  }, [agentStates]);

  const actorById = useMemo(() => {
    const values = new Map<string, Actor>();
    for (const actor of actors || []) {
      const id = String(actor.id || "").trim();
      const title = String(actor.title || "").trim();
      for (const key of [id, title, id.toLowerCase(), title.toLowerCase()]) {
        if (key && !values.has(key)) values.set(key, actor);
      }
    }
    return values;
  }, [actors]);

  const actorDisplayNames = useActorDisplayNameMap(actors);
  const displayNameMap = useMemo(() => {
    const values = new Map(actorDisplayNames);
    for (const [id, name] of buildGroupBridgeDisplayNameMap(messages)) values.set(id, name);
    return values;
  }, [actorDisplayNames, messages]);

  return { messageTextById, agentStateById, actorById, displayNameMap };
}
