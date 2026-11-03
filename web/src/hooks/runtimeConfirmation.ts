import i18n from "../i18n";
import { useGroupStore, useUIStore } from "../stores";
import { requestWorkspaceNavigation } from "../stores/workspaceNavigation";
import type { Actor } from "../types";

export const RUNTIME_CONFIRMATION_REFRESH_EVENTS = new Set([
  "headless.session.started",
  "headless.session.stopped",
  "headless.session.failed",
  "headless.session.exited",
  "headless.session.resume_failed",
  "headless.thread.resume_failed",
  "headless.turn.failed",
  "headless.turn.cancelled",
]);

export function runtimeConfirmationRequired(result: unknown, actor?: Actor): boolean {
  const value = result as
    | { runtime_confirmation_required?: boolean; actor?: Actor }
    | null
    | undefined;
  if (typeof value?.runtime_confirmation_required === "boolean")
    return value.runtime_confirmation_required;
  if (typeof value?.actor?.runtime_confirmation_required === "boolean")
    return value.actor.runtime_confirmation_required;
  return actor?.runtime_confirmation_required === true;
}

/**
 * After the user's own start, show the Actor whose Claude prompt waits for them, without
 * answering it. Where the user still is where they acted, open its terminal, whose notice
 * says what to do; where they have moved on in the same Group, say so with a way there;
 * in another Group, only say so. Returns whether the Actor awaits confirmation.
 */
export function revealRuntimeConfirmation(
  groupId: string,
  actorId: string,
  expectedTab: string,
  result?: unknown,
  options: { noticePrefix?: string } = {},
): boolean {
  const state = useGroupStore.getState();
  const actor =
    state.selectedGroupId === groupId
      ? state.actors.find((item) => item.id === actorId)
      : undefined;
  if (!runtimeConfirmationRequired(result, actor)) return false;
  const ui = useUIStore.getState();
  const sameGroup = () => useGroupStore.getState().selectedGroupId === groupId;
  const openTerminal = () =>
    requestWorkspaceNavigation(() => {
      const group = useGroupStore.getState();
      if (group.selectedGroupId !== groupId || !group.actors.some((item) => item.id === actorId))
        return;
      const current = useUIStore.getState();
      current.setChatMobileSurface(groupId, "messages");
      current.setActiveTab(actorId);
    });
  if (sameGroup() && ui.activeTab === expectedTab) {
    if (options.noticePrefix) ui.showNotice({ message: options.noticePrefix });
    openTerminal();
    return true;
  }
  const label = sameGroup()
    ? actor?.title || actorId
    : `${state.groups.find((group) => group.group_id === groupId)?.title || groupId} / ${actor?.title || actorId}`;
  const notice = {
    message: [options.noticePrefix, i18n.t("actors:runtimeConfirmationRequired", { label })]
      .filter(Boolean)
      .join("\n"),
    ...(sameGroup()
      ? { actionLabel: i18n.t("actors:runtimeConfirmationOpenTerminal"), onAction: openTerminal }
      : {}),
  };
  ui.showNotice(notice);
  if (notice.onAction) {
    // A notice with an action stays until used; once the prompt is answered it has nothing
    // left to offer, so it goes.
    const stop = useGroupStore.subscribe((group) => {
      if (useUIStore.getState().notice !== notice) return stop();
      const current =
        group.selectedGroupId === groupId
          ? group.actors.find((item) => item.id === actorId)
          : undefined;
      if (current?.runtime_confirmation_required) return;
      stop();
      useUIStore.getState().dismissNotice();
    });
  }
  return true;
}
