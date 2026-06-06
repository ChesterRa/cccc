import type { Actor } from "../types";

export function actorHasRuntimeResumeFailure(actor: Pick<Actor, "runtime_session_status">): boolean {
  return String(actor.runtime_session_status || "").trim().toLowerCase() === "resume_failed";
}

export function shouldFetchStoppedTerminalTail(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  isActorBusy: boolean;
}): boolean {
  return Boolean(args.activated && !args.isRunning && !args.isHeadless && args.groupId && args.actorId && !args.isActorBusy);
}
