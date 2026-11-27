import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

/** Shared attention predicate for both desktop sidebar Dock and mobile top bar. */
export function codexVoiceNeedsAttention({
  controller,
  chetAttention,
  unattended,
}: {
  controller: CodexVoiceSessionController;
  chetAttention: boolean;
  unattended: boolean;
}): boolean {
  return Boolean(
    controller.error ||
    controller.refreshError ||
    controller.playbackBlocked ||
    controller.analyst?.warning ||
    controller.analyst?.phase === "needs_attention" ||
    (controller.analyst?.permissions?.length ?? 0) > 0 ||
    chetAttention ||
    // Unattended copies wait for a Chet that cannot run; the panel says why.
    (unattended && controller.readiness?.analyst_runtime_available === false),
  );
}
