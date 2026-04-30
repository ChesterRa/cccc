import type { Actor } from "../../types";
import { getEffectiveActorRunner } from "../../utils/headlessRuntimeSupport";
import type { LiveWorkCard } from "./liveWorkCards";

export type RuntimeDockRunner = "pty" | "headless";

export type RuntimeDockItem = {
  actor: Actor;
  actorId: string;
  actorLabel: string;
  runtime: string;
  runner: RuntimeDockRunner;
  unreadCount: number;
  webModelQueuedCount: number;
  liveWorkCard: LiveWorkCard | null;
};

function getRuntimeDockRunner(actor: Actor): RuntimeDockRunner {
  return getEffectiveActorRunner(actor);
}

export function buildRuntimeDockItems(args: {
  actors: Actor[];
  liveWorkCards: LiveWorkCard[];
}): RuntimeDockItem[] {
  const liveWorkCardByActorId = new Map<string, LiveWorkCard>();
  for (const card of Array.isArray(args.liveWorkCards) ? args.liveWorkCards : []) {
    const actorId = String(card.actorId || "").trim();
    if (!actorId || liveWorkCardByActorId.has(actorId)) continue;
    liveWorkCardByActorId.set(actorId, card);
  }

  const items: RuntimeDockItem[] = [];
  for (const actor of Array.isArray(args.actors) ? args.actors : []) {
    const actorId = String(actor.id || "").trim();
    if (!actorId) continue;
    items.push({
      actor,
      actorId,
      actorLabel: String(actor.title || actorId).trim() || actorId,
      runtime: String(actor.runtime || "custom").trim() || "custom",
      runner: getRuntimeDockRunner(actor),
      unreadCount: Math.max(0, Number(actor.unread_count || 0)),
      webModelQueuedCount: Math.max(0, Number(actor.web_model_queued_count || 0)),
      liveWorkCard: liveWorkCardByActorId.get(actorId) || null,
    });
  }
  return items;
}
