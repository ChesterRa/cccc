import i18n from "../i18n";
import { useUIStore } from "../stores";
import type { ActorRuntimeActionResult } from "../services/api/actors";
import { SESSION_REPLACED_REASONS } from "../components/AgentTab.model";

/** A start that began a new conversation instead of the saved one says so once, at the action. */
export function noticeRuntimeSessionReplaced(name: string, result: unknown): void {
  if (!result || typeof result !== "object") return;
  const replaced = (result as ActorRuntimeActionResult).runtime_session_replaced;
  if (!replaced || typeof replaced !== "object") return;
  const reported = Array.isArray(replaced.reasons) ? replaced.reasons : [];
  // The daemon's list is open: a reason this build has no words for is left out.
  const reasons = SESSION_REPLACED_REASONS.filter((reason) => reported.includes(reason));
  const parts = [i18n.t("actors:runtimeSessionReplacedNow", { name })];
  if (reasons.length) {
    parts.push(
      i18n.t("actors:runtimeSessionReplacedReasons", {
        reasons: reasons
          .map((reason) => i18n.t(`actors:runtimeSessionReplacedReason.${reason}`))
          .join(i18n.t("actors:runtimeSessionReplacedReasonSeparator")),
      }),
    );
  }
  useUIStore.getState().showNotice({ message: parts.join(" ") });
}
