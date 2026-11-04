import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertIcon, ChevronDownIcon } from "../../components/Icons";
import { useGroupStore, useUIStore } from "../../stores";
import { hasComposerDraft, useComposerStore, type FailedSend } from "../../stores/useComposerStore";
import { requestWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { classNames } from "../../utils/classNames";

/** Where a send waits relative to the open Group: its own, another listed one, or one no longer listed. */
type Origin = "here" | "other" | "unavailable";

function firstLine(text: string): string {
  return text.trim().split("\n", 1)[0] ?? "";
}

function groupListed(groupId: string): boolean {
  return useGroupStore.getState().groups.some((group) => group.group_id === groupId);
}

/**
 * Every send in this page whose result never came back, listed in whichever Group is open (or in
 * place of the composer when none is), so none is stranded in a Group the user left or an archived
 * one. Collapsed to one line.
 *
 * Content shows only in the send's own Group, as drafts do; elsewhere a row only says where it
 * waits. A send whose Group is no longer listed can only be discarded: the list can lose a Group
 * to a deletion or to a sign-in with other access, and this page cannot tell which.
 */
export function ComposerFailedSends({ groupId, isDark }: { groupId: string; isDark: boolean }) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const failedSendsByGroup = useComposerStore((state) => state.failedSendsByGroup);
  const draftPresent = useComposerStore(hasComposerDraft);
  const restoreFailedSend = useComposerStore((state) => state.restoreFailedSend);
  const discardFailedSend = useComposerStore((state) => state.discardFailedSend);
  const groups = useGroupStore((state) => state.groups);
  const groupsLoaded = useGroupStore((state) => state.groupsLoaded);

  // This Group's sends first, then the others in the order their Groups first failed.
  const failed = [
    ...(failedSendsByGroup[groupId] ?? []),
    ...Object.entries(failedSendsByGroup)
      .filter(([origin]) => origin !== groupId)
      .flatMap(([, items]) => items),
  ];
  if (failed.length === 0) return null;

  const groupTitle = (id: string) => groups.find((group) => group.group_id === id)?.title || id;
  // "Not listed" wins over "this Group": the list can drop a Group before the selection follows.
  const originOf = (item: FailedSend): Origin =>
    groupsLoaded && !groups.some((group) => group.group_id === item.originGroupId)
      ? "unavailable"
      : item.originGroupId === groupId
        ? "here"
        : "other";

  const openOrigin = (item: FailedSend) => {
    // Navigation can wait for unsaved work to be confirmed; the Group may be gone by then.
    const proceed = () => {
      if (!groupListed(item.originGroupId)) return;
      useGroupStore.getState().setSelectedGroupId(item.originGroupId);
      useUIStore.getState().setActiveTab("chat");
    };
    if (groupListed(item.originGroupId)) requestWorkspaceNavigation(proceed);
  };
  const actionClass = classNames(
    // Discarding cannot be undone, so touch screens get full-size, spaced targets.
    "flex-shrink-0 rounded px-1.5 py-0.5 transition-colors disabled:cursor-not-allowed disabled:opacity-40 pointer-coarse:min-h-11 pointer-coarse:px-3",
    "text-[var(--color-text-secondary)] enabled:hover:text-[var(--color-text-primary)]",
    isDark ? "enabled:hover:bg-white/[0.08]" : "enabled:hover:bg-black/[0.06]",
  );
  // Only restoring needs the empty composer.
  const blocked = draftPresent && failed.some((item) => originOf(item) === "here");

  return (
    <details
      data-composer-failed-sends
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
      className={classNames(
        "mb-2.5 rounded-lg border text-xs text-[var(--color-text-secondary)]",
        isDark ? "border-white/[0.06] bg-white/[0.035]" : "border-black/[0.05] bg-black/[0.025]",
      )}
    >
      <summary className="flex cursor-pointer list-none items-center gap-1.5 px-2.5 py-1 [&::-webkit-details-marker]:hidden">
        <AlertIcon
          size={12}
          className="flex-shrink-0 text-[var(--color-accent-warning)]"
          aria-hidden="true"
        />
        <span className="min-w-0 flex-1 truncate">
          {t("failedSendsSummary", { count: failed.length })}
        </span>
        <ChevronDownIcon
          size={12}
          aria-hidden="true"
          className={classNames(
            "flex-shrink-0 opacity-55 transition-transform motion-reduce:transition-none",
            open && "rotate-180",
          )}
        />
      </summary>
      <p className="px-2.5 pb-1">{t("failedSendHint")}</p>
      <div className="max-h-36 overflow-y-auto pb-0.5">
        {failed.map((item) => {
          const origin = originOf(item);
          const here = origin === "here";
          const preview = here ? firstLine(item.composerText) : "";
          // A reply to a message from another Group on this instance names that Group on its target.
          const destination = item.replyTarget?.remoteDstGroupId || item.destGroupId;
          return (
            <div
              key={`${item.originGroupId}:${item.id}`}
              data-failed-send={item.id}
              data-failed-send-origin={origin}
              className="flex items-center gap-1.5 px-2.5 py-0.5 pointer-coarse:gap-3"
            >
              <span className="min-w-0 flex-1 truncate">
                <span className="mr-1 text-[var(--color-text-tertiary)]">{t("failedSendTag")}</span>
                {origin === "other" ? (
                  <span className="mr-1">
                    {t("failedSendFrom", { group: groupTitle(item.originGroupId) })}
                  </span>
                ) : origin === "unavailable" ? (
                  <span className="mr-1">{t("failedSendSourceUnavailable")}</span>
                ) : destination && destination !== item.originGroupId ? (
                  <span className="mr-1">
                    {t("failedSendTo", { group: groupTitle(destination) })}
                  </span>
                ) : null}
                {preview ? (
                  <span className="text-[var(--color-text-primary)]">{preview}</span>
                ) : null}
                {here && item.composerFiles.length > 0 ? (
                  <span className="ml-1 text-[var(--color-text-tertiary)]">
                    {t("failedSendAttachments", { count: item.composerFiles.length })}
                  </span>
                ) : null}
              </span>
              {origin === "here" ? (
                <button
                  type="button"
                  className={actionClass}
                  disabled={draftPresent}
                  onClick={() => restoreFailedSend(groupId, item.id)}
                >
                  {t("failedSendRestore")}
                </button>
              ) : origin === "other" ? (
                // Decide on it where its content is shown.
                <button type="button" className={actionClass} onClick={() => openOrigin(item)}>
                  {t("failedSendOpen")}
                </button>
              ) : null}
              {origin !== "other" ? (
                <button
                  type="button"
                  className={actionClass}
                  onClick={() => discardFailedSend(item.originGroupId, item.id)}
                >
                  {t("failedSendDiscard")}
                </button>
              ) : null}
            </div>
          );
        })}
      </div>
      {failed.some((item) => originOf(item) === "unavailable") ? (
        <p className="px-2.5 pb-1">{t("failedSendUnavailableHint")}</p>
      ) : null}
      {blocked ? <p className="px-2.5 pb-1">{t("failedSendRestoreBlocked")}</p> : null}
    </details>
  );
}
