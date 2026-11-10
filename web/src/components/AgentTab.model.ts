import type { Actor } from "../types";
import { supportsAcpMode } from "../types";

export function actorSupportsNewSession(
  runtime: Actor["runtime"],
  mode?: Actor["runtime_mode"],
): boolean {
  if (supportsAcpMode(runtime) && mode === "acp") return true;
  return ["claude", "codex", "grok", "pi", "opencode", "kilo"].includes(
    String(runtime || "")
      .trim()
      .toLowerCase(),
  );
}

const SESSION_REPLACED_REASONS = [
  "runtime",
  "transport",
  "record_version",
  "workspace",
  "command",
  "model",
  "launch_path",
  "cccc_cli",
  "launch_identity",
] as const;

/** A start of this agent did not resume the conversation saved before it; `at` is when that was recorded. */
export function actorSessionReplacement(
  actor: Pick<Actor, "runtime_session_replaced_at" | "runtime_session_replaced_reasons">,
): { at: string; reasons: (typeof SESSION_REPLACED_REASONS)[number][] } | null {
  const at = String(actor.runtime_session_replaced_at || "").trim();
  if (!at) return null;
  const reported = Array.isArray(actor.runtime_session_replaced_reasons)
    ? actor.runtime_session_replaced_reasons
    : [];
  // The daemon's list is open: a reason this build has no words for is left out.
  return { at, reasons: SESSION_REPLACED_REASONS.filter((reason) => reported.includes(reason)) };
}

export function actorHasRuntimeResumeFailure(
  actor: Pick<Actor, "runtime_session_status">,
): boolean {
  return (
    String(actor.runtime_session_status || "")
      .trim()
      .toLowerCase() === "resume_failed"
  );
}

export function shouldFetchStoppedTerminalTail(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  isActorBusy: boolean;
}): boolean {
  return Boolean(
    args.activated &&
    !args.isRunning &&
    !args.isHeadless &&
    args.groupId &&
    args.actorId &&
    !args.isActorBusy,
  );
}

export function shouldReconcileStoppedActorStatus(args: {
  activated: boolean;
  isVisible: boolean;
  isRunning: boolean;
  isActorEnabled: boolean;
  isActorBusy: boolean;
}): boolean {
  return Boolean(
    args.activated && args.isVisible && !args.isRunning && args.isActorEnabled && !args.isActorBusy,
  );
}
