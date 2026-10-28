import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { MessagesSquare } from "lucide-react";
import type { Actor, LocalSessionSource } from "../../types";
import {
  createLocalConversation,
  fetchLocalSessions,
  joinLocalSessions,
  resumeLocalSessionAccess,
} from "../../services/api/localSessions";
import { fetchActors } from "../../services/api/actors";
import { useGroupStore, useUIStore } from "../../stores";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { ActorAvatar } from "../../components/ActorAvatar";

const MAX_SELECTED_SESSIONS = 16;

export function LocalSessionsButton({
  collapsed,
  isDark,
  onSelectGroup,
}: {
  collapsed: boolean;
  isDark: boolean;
  onSelectGroup: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button
        variant="ghost"
        className="mt-2 w-full justify-start gap-2"
        onClick={() => setOpen(true)}
        title={t("localSessions.connect")}
        aria-label={t("localSessions.connect")}
      >
        <MessagesSquare size={18} />
        {!collapsed && t("localSessions.connect")}
      </Button>
      {open &&
        createPortal(
          <LocalSessionsDialog
            isDark={isDark}
            onClose={() => setOpen(false)}
            onSelectGroup={onSelectGroup}
          />,
          document.body,
        )}
    </>
  );
}

function LocalSessionsDialog({
  isDark,
  onClose,
  onSelectGroup,
}: {
  isDark: boolean;
  onClose: () => void;
  onSelectGroup: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const { modalRef } = useModalA11y(true, onClose);
  const groups = useGroupStore((state) => state.groups);
  const [targetGroupId, setTargetGroupId] = useState("");
  const [sources, setSources] = useState<LocalSessionSource[]>([]);
  const [selected, setSelected] = useState<LocalSessionSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [membership, setMembership] = useState<{
    groupId: string;
    actors: Actor[];
    loaded: boolean;
    failed: boolean;
  }>({ groupId: "", actors: [], loaded: false, failed: false });
  // Setup guidance opens by itself only when the first list is empty; after
  // that the reader decides, except when asking to see troubleshooting.
  const [setupOpen, setSetupOpen] = useState(false);
  const [troubleshootingOpen, setTroubleshootingOpen] = useState(false);
  const [troubleshootingRequest, setTroubleshootingRequest] = useState(0);
  const setupDecided = useRef(false);
  const troubleshootingRef = useRef<HTMLDetailsElement>(null);
  const targetRef = useRef(targetGroupId);
  targetRef.current = targetGroupId;
  const membersRequest = useRef(0);
  const submitting = useRef(false);
  const alive = useRef(true);
  const loadMembers = useCallback(async (groupId: string) => {
    const request = ++membersRequest.current;
    try {
      const response = await fetchActors(groupId, false, { noCache: true });
      if (!alive.current || targetRef.current !== groupId || membersRequest.current !== request)
        return;
      const actors = response.ok ? response.result.actors : [];
      setMembership({ groupId, actors, loaded: true, failed: !response.ok });
      if (response.ok)
        setSelected((previous) =>
          previous.filter(
            (source) => !actors.some((actor) => actor.external_session_id === source.source_id),
          ),
        );
    } catch {
      if (alive.current && targetRef.current === groupId && membersRequest.current === request) {
        setMembership({ groupId, actors: [], loaded: true, failed: true });
      }
    }
  }, []);
  useEffect(() => {
    setNotice("");
    if (!targetGroupId) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    setMembership({ groupId: targetGroupId, actors: [], loaded: false, failed: false });
    const load = async () => {
      await loadMembers(targetGroupId);
      if (!cancelled) timer = setTimeout(() => void load(), 3000);
    };
    void load();
    return () => {
      cancelled = true;
      membersRequest.current += 1;
      clearTimeout(timer);
    };
  }, [targetGroupId, loadMembers]);
  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      const response = await fetchLocalSessions();
      if (cancelled) return;
      setLoading(false);
      if (response.ok) {
        setError("");
        setSources(response.result.sources);
        if (!setupDecided.current) {
          setupDecided.current = true;
          if (!response.result.sources.length) setSetupOpen(true);
        }
        setSelected((previous) =>
          previous.map(
            (source) =>
              response.result.sources.find((latest) => latest.source_id === source.source_id) || {
                ...source,
                connected: false,
              },
          ),
        );
      } else setError(response.error.message);
      timer = setTimeout(() => {
        void load();
      }, 3000);
    };
    void load();
    return () => {
      cancelled = true;
      alive.current = false;
      clearTimeout(timer);
    };
  }, []);
  useEffect(() => {
    if (!troubleshootingRequest) return;
    troubleshootingRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [troubleshootingRequest]);
  const showTroubleshooting = () => {
    setSetupOpen(true);
    setTroubleshootingOpen(true);
    setTroubleshootingRequest((count) => count + 1);
  };
  const selectedIds = selected.map((source) => source.source_id);
  const unavailableCount = selected.filter((source) => !source.connected).length;
  const displayedSources = [
    ...sources,
    ...selected.filter((source) => !sources.some((item) => item.source_id === source.source_id)),
  ];
  const anyUnavailable = displayedSources.some((source) => !source.connected);
  const firstSelected = selected[0];
  const joining = !!targetGroupId;
  const membersReady =
    !joining || (membership.groupId === targetGroupId && membership.loaded && !membership.failed);
  const members = new Map(
    (joining && membership.groupId === targetGroupId ? membership.actors : [])
      .filter((actor) => actor.external_session_id)
      .map((actor) => [actor.external_session_id!, actor]),
  );
  const targetUnavailable = joining && !groups.some((group) => group.group_id === targetGroupId);
  const canSubmit =
    !busy &&
    !loading &&
    !error &&
    !targetUnavailable &&
    membersReady &&
    selected.length > 0 &&
    selected.length <= MAX_SELECTED_SESSIONS &&
    unavailableCount === 0;
  const start = async () => {
    if (!canSubmit || submitting.current) return;
    submitting.current = true;
    setBusy(true);
    try {
      const response = joining
        ? await joinLocalSessions(targetGroupId, selectedIds)
        : await createLocalConversation(selectedIds);
      if (!response.ok) {
        useUIStore.getState().showError(response.error.message);
        return;
      }
      // A completed user mutation still refreshes Groups if the dialog was closed.
      await useGroupStore.getState().refreshGroups();
      if (joining) {
        const store = useGroupStore.getState();
        if (store.selectedGroupId === response.result.group_id) {
          store.setGroupDoc(response.result.group);
        }
        // Re-selecting the current Group does not reload its actors.
        await store.refreshActors(response.result.group_id);
      }
      if (alive.current) {
        onSelectGroup(response.result.group_id);
        onClose();
      }
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setBusy(false);
    }
  };
  const resume = async (actor: Actor, source: LocalSessionSource) => {
    if (busy || submitting.current || !targetGroupId) return;
    const groupId = targetGroupId;
    submitting.current = true;
    setBusy(true);
    setNotice("");
    try {
      const response = await resumeLocalSessionAccess(groupId, actor.id, source.source_id);
      if (!response.ok) useUIStore.getState().showError(response.error.message);
      else if (alive.current && targetRef.current === groupId)
        setNotice(t("localSessions.accessRestored", { name: actor.title || actor.id }));
      await loadMembers(groupId);
      await Promise.all([
        useGroupStore.getState().refreshActors(groupId),
        useGroupStore.getState().refreshGroups(),
      ]);
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setBusy(false);
    }
  };
  return (
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      titleId="local-sessions-title"
      title={t("localSessions.title")}
      closeAriaLabel={t("common:close")}
      modalRef={modalRef}
      panelClassName="w-full sm:max-w-xl sm:max-h-[80dvh]"
      surface="solid"
      footerActions={
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <p role="status" className="text-xs text-[var(--color-text-secondary)]">
            {t("localSessions.selectedCount", {
              count: selected.length,
              limit: MAX_SELECTED_SESSIONS,
            })}
          </p>
          <Button
            disabled={!canSubmit}
            aria-busy={busy}
            onClick={() => {
              void start();
            }}
          >
            {t(
              joining
                ? busy
                  ? "localSessions.joining"
                  : "localSessions.join"
                : busy
                  ? "localSessions.creating"
                  : "localSessions.create",
            )}
          </Button>
        </div>
      }
    >
      <div className="min-h-0 overflow-y-auto p-4 sm:p-6" aria-busy={loading || busy}>
        <p className="mb-4 text-sm text-[var(--color-text-secondary)]">{t("localSessions.hint")}</p>
        <div className="mb-4 space-y-2">
          <div className="text-xs font-medium text-[var(--color-text-muted)]">
            {t("localSessions.target")}
          </div>
          <SelectMenu
            ariaLabel={t("localSessions.target")}
            value={targetGroupId}
            onChange={setTargetGroupId}
            disabled={busy}
            placeholder={t("localSessions.targetUnavailable")}
            align="start"
            className="w-full"
            contentClassName="w-[var(--radix-popover-trigger-width)]"
            options={[
              { value: "", label: t("localSessions.newConversation") },
              ...groups.map((group) => ({
                value: group.group_id,
                label: group.title || group.group_id,
              })),
            ]}
          />
        </div>
        <p className="mb-4 text-sm text-[var(--color-text-secondary)]">
          {t(joining ? "localSessions.joinHint" : "localSessions.groupHint")}
        </p>
        {error && (
          <p role="alert" className="mb-3 text-sm text-red-600">
            {t("localSessions.loadFailed")}
            <span className="block text-xs">{error}</span>
          </p>
        )}
        {targetUnavailable && (
          <p role="alert" className="mb-3 text-sm text-red-600">
            {t("localSessions.targetUnavailable")}
          </p>
        )}
        {joining && !membersReady ? (
          <p role={membership.failed ? "alert" : "status"} className="mb-3 text-sm">
            {t(membership.failed ? "localSessions.membersFailed" : "common:loading")}
          </p>
        ) : null}
        {notice ? (
          <p role="status" className="mb-3 text-sm">
            {notice}
          </p>
        ) : null}
        {unavailableCount > 0 && (
          <p role="alert" className="mb-3 text-sm text-amber-700 dark:text-amber-300">
            {t("localSessions.selectionUnavailable", { count: unavailableCount })}
          </p>
        )}
        {!loading && anyUnavailable ? (
          <p className="mb-3 text-xs text-[var(--color-text-secondary)]">
            {t("localSessions.unavailableHint")}{" "}
            <button
              type="button"
              className="underline underline-offset-2"
              onClick={showTroubleshooting}
            >
              {t("localSessions.showTroubleshooting")}
            </button>
          </p>
        ) : null}
        {loading ? (
          <p role="status">{t("common:loading")}</p>
        ) : displayedSources.length ? (
          <div className="divide-y divide-[var(--glass-border-subtle)]">
            {displayedSources.map((source) => {
              const checked = selectedIds.includes(source.source_id);
              const member = members.get(source.source_id);
              const origin =
                source.runtime === "codex" &&
                (source.origin === "cli" || source.origin === "vscode")
                  ? source.origin === "cli"
                    ? "CLI"
                    : "VS Code"
                  : null;
              return (
                <div
                  key={source.source_id}
                  className="flex min-h-16 flex-wrap items-center gap-3 py-3 sm:flex-nowrap"
                >
                  <label className="flex min-w-0 flex-1 basis-full cursor-pointer items-center gap-3 sm:basis-auto">
                    <input
                      type="checkbox"
                      className="h-4 w-4 shrink-0 accent-[var(--color-text-primary)]"
                      checked={checked}
                      disabled={
                        busy ||
                        !membersReady ||
                        !!member ||
                        (!checked &&
                          (!source.connected || selected.length >= MAX_SELECTED_SESSIONS))
                      }
                      onChange={(event) =>
                        setSelected((previous) =>
                          event.target.checked
                            ? [...previous, source]
                            : previous.filter((item) => item.source_id !== source.source_id),
                        )
                      }
                    />
                    <ActorAvatar
                      runtime={source.runtime}
                      title={source.title}
                      isDark={isDark}
                      sizeClassName="h-8 w-8"
                    />
                    <span className="min-w-0">
                      <span className="flex min-w-0 items-center gap-2">
                        <span className="truncate text-sm font-medium" title={source.title}>
                          {source.title}
                        </span>
                        {origin ? (
                          <span
                            className="shrink-0 rounded border border-[var(--glass-border-subtle)] px-1.5 py-0.5 text-xs text-[var(--color-text-secondary)]"
                            title={t("localSessions.originHint", { origin })}
                          >
                            {origin}
                          </span>
                        ) : null}
                      </span>
                      <span
                        className="block truncate text-xs text-[var(--color-text-secondary)]"
                        title={source.cwd}
                      >
                        {source.cwd}
                      </span>
                      {!source.connected && (
                        <span className="block text-xs text-amber-700 dark:text-amber-300">
                          {t("localSessions.unavailable")}
                        </span>
                      )}
                      {member ? (
                        <span className="block text-xs text-[var(--color-text-secondary)]">
                          {t(
                            member.enabled === false
                              ? "localSessions.memberPaused"
                              : "localSessions.memberJoined",
                          )}
                        </span>
                      ) : null}
                    </span>
                  </label>
                  {member?.enabled === false ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="ml-auto max-sm:min-h-11 sm:ml-0"
                      disabled={busy}
                      onClick={() => void resume(member, source)}
                    >
                      {t("actors:dotsResume")}
                    </Button>
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : !error ? (
          <p className="text-sm">{t("localSessions.empty")}</p>
        ) : null}
        {!joining && firstSelected ? (
          <section
            className="mt-4 space-y-2 text-xs text-[var(--color-text-secondary)]"
            aria-label={t("localSessions.newGroupDefaults")}
          >
            <h3 className="font-medium text-[var(--color-text-primary)]">
              {t("localSessions.newGroupDefaults")}
            </h3>
            <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
              <dt className="self-center">{t("actors:groupRoleForeman")}</dt>
              <dd className="min-w-0 break-words">
                {selected.length > 1 ? (
                  <SelectMenu
                    ariaLabel={t("actors:groupRoleForeman")}
                    value={firstSelected.source_id}
                    options={selected.map((source) => ({
                      value: source.source_id,
                      label: source.title,
                    }))}
                    onChange={(sourceId) =>
                      setSelected((previous) => {
                        const foreman = previous.find((source) => source.source_id === sourceId);
                        return foreman
                          ? [foreman, ...previous.filter((source) => source.source_id !== sourceId)]
                          : previous;
                      })
                    }
                    disabled={busy}
                    align="start"
                    className="w-full"
                    contentClassName="w-[var(--radix-popover-trigger-width)]"
                  />
                ) : (
                  firstSelected.title
                )}
              </dd>
              <dt>{t("actors:workingDirectory")}</dt>
              <dd className="break-all">{firstSelected.cwd}</dd>
            </dl>
            {selected.length > 1 ? <p>{t("localSessions.foremanDirectoryHint")}</p> : null}
          </section>
        ) : null}
        <details
          className="mt-5 text-sm text-[var(--color-text-secondary)]"
          open={setupOpen}
          onToggle={(event) => setSetupOpen(event.currentTarget.open)}
        >
          <summary className="cursor-pointer">{t("localSessions.setup")}</summary>
          <p className="my-2">{t("localSessions.setupHint")}</p>
          <p className="my-2">{t("localSessions.setupTerminal")}</p>
          <p className="my-2">{t("localSessions.setupHost")}</p>
          <pre className="overflow-x-auto text-xs">
            {
              "cccc sessions enable --runtime codex\ncccc sessions enable --runtime claude\ncccc sessions enable --runtime pi\ncccc sessions enable --runtime opencode"
            }
          </pre>
          <p className="my-2">{t("localSessions.setupNext")}</p>
          <ul className="my-2 list-disc space-y-2 pl-5">
            <li>{t("localSessions.setupCodex")}</li>
            <li>{t("localSessions.setupClaude")}</li>
            <li>{t("localSessions.setupPi")}</li>
            <li>{t("localSessions.setupOpencode")}</li>
          </ul>
          <p className="my-2">{t("localSessions.setupStart")}</p>
          <blockquote className="break-words border-l-2 border-[var(--color-border-secondary)] pl-3">
            {t("localSessions.setupExample")}
          </blockquote>
          <p className="my-2">{t("localSessions.setupConversationHint")}</p>
          <details
            ref={troubleshootingRef}
            className="my-2"
            open={troubleshootingOpen}
            onToggle={(event) => setTroubleshootingOpen(event.currentTarget.open)}
          >
            <summary className="cursor-pointer">{t("localSessions.troubleshooting")}</summary>
            <p className="my-2">{t("localSessions.setupCheck")}</p>
            <pre className="overflow-x-auto text-xs">cccc sessions list</pre>
            <ul className="my-2 list-disc space-y-2 pl-5">
              <li>{t("localSessions.setupReload")}</li>
              <li>{t("localSessions.troubleshootCodex")}</li>
              <li>{t("localSessions.troubleshootClaude")}</li>
            </ul>
          </details>
          <a
            className="mt-3 inline-block underline underline-offset-2"
            href="https://chesterra.github.io/cccc/guide/local-sessions"
            target="_blank"
            rel="noopener noreferrer"
          >
            {t("localSessions.setupGuide")}
          </a>
        </details>
      </div>
    </ModalFrame>
  );
}
