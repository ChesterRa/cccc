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

/** Reveal the existing PTY without answering its prompt or changing its authority. */
export function revealRuntimeConfirmation(
  groupId: string,
  actorId: string,
  expectedTab: string,
  result?: unknown,
  options: {
    autoOpen?: boolean;
    noticePrefix?: string;
    canNavigate?: () => boolean;
    beforeNavigate?: () => void;
  } = {},
): boolean {
  const state = useGroupStore.getState();
  const actor =
    state.selectedGroupId === groupId
      ? state.actors.find((item) => item.id === actorId)
      : undefined;
  if (!runtimeConfirmationRequired(result, actor)) return false;
  const canNavigate = () =>
    useGroupStore.getState().selectedGroupId === groupId &&
    useUIStore.getState().activeTab === expectedTab &&
    (options.canNavigate?.() ?? true);
  const openTerminal = (explicit = false) => {
    if ((!explicit && !canNavigate()) || options.canNavigate?.() === false) return;
    requestWorkspaceNavigation(() => {
      const current = useGroupStore.getState();
      const ui = useUIStore.getState();
      if ((!explicit && !canNavigate()) || options.canNavigate?.() === false) return;
      const enter = () => {
        const group = useGroupStore.getState();
        if (group.selectedGroupId !== groupId || !group.actors.some((item) => item.id === actorId))
          return;
        if (options.canNavigate?.() === false) return;
        options.beforeNavigate?.();
        useUIStore.getState().setChatMobileSurface(groupId, "messages");
        useUIStore.getState().setActiveTab(actorId);
      };
      if (current.selectedGroupId === groupId) {
        enter();
        return;
      }
      if (!explicit || !current.groups.some((group) => group.group_id === groupId)) return;
      // Group selection owns loading and resets its tab. Open the Actor only after
      // that visit's hydration, and retire the request if the user leaves again.
      const unsubscribe = useGroupStore.subscribe((group) => {
        if (group.selectedGroupId !== groupId) {
          unsubscribe();
        } else if (!group.selectedGroupActorsHydrating) {
          unsubscribe();
          if (useUIStore.getState().activeTab === "chat") enter();
        }
      });
      ui.setActiveTab("chat");
      current.setSelectedGroupId(groupId);
    });
  };
  const offerNavigation = options.autoOpen === false || !canNavigate();
  const actionTarget =
    state.selectedGroupId !== groupId
      ? ` · ${state.groups.find((group) => group.group_id === groupId)?.title || groupId} / ${actor?.title || actorId}`
      : "";
  useUIStore
    .getState()
    .showNotice({
      message: [
        options.noticePrefix,
        i18n.t("actors:runtimeConfirmationRequired", { label: actor?.title || actorId }),
      ]
        .filter(Boolean)
        .join("\n"),
      ...(offerNavigation
        ? {
            actionLabel: `${i18n.t("actors:runtimeConfirmationOpenTerminal")}${actionTarget}`,
            onAction: () => openTerminal(true),
          }
        : {}),
    });
  if (!offerNavigation) openTerminal();
  return true;
}
