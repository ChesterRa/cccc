import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CloseIcon, FolderIcon, GlobeIcon, PlusIcon, PowerIcon, RefreshIcon, TrashIcon } from "../../Icons";
import type {
  GroupSpaceBinding,
  GroupSpaceProviderAuthStatus,
  GroupSpaceRemoteSpace,
  GroupSpaceStatus,
} from "../../../types";
import * as api from "../../../services/api";
import { ProjectedBrowserSurfacePanel } from "../../browser/ProjectedBrowserSurfacePanel";
import { SelectCombobox } from "../../SelectCombobox";
import {
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { resolveNotebookSpacesAfterLoad, shouldRefreshNotebookSpaces } from "./groupSpaceState";

interface GroupSpaceTabProps {
  isDark: boolean;
  groupId?: string;
  isActive?: boolean;
}

type NotebookLane = "work" | "memory";
type LaneVisualState = "unbound" | "active" | "saved";

const NOTEBOOK_LANES: NotebookLane[] = ["work", "memory"];

function normalizeNotebookSpaces(raw: unknown): GroupSpaceRemoteSpace[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((item) => Boolean(String((item as GroupSpaceRemoteSpace | null)?.remote_space_id || "").trim()))
    .map((item) => ({
      remote_space_id: String((item as GroupSpaceRemoteSpace | null)?.remote_space_id || "").trim(),
      title: String((item as GroupSpaceRemoteSpace | null)?.title || "").trim(),
      created_at: String((item as GroupSpaceRemoteSpace | null)?.created_at || "").trim(),
      is_owner: Boolean((item as GroupSpaceRemoteSpace | null)?.is_owner),
    }));
}

function resolveDraftNotebookId(
  previousDraft: string,
  boundRemoteId: string,
  options: GroupSpaceRemoteSpace[]
): string {
  const bound = String(boundRemoteId || "").trim();
  if (bound) return bound;
  const previous = String(previousDraft || "").trim();
  if (previous && options.some((item) => String(item.remote_space_id || "").trim() === previous)) {
    return previous;
  }
  return String(options[0]?.remote_space_id || "").trim();
}

function mergeNotebookOptions(
  spaces: GroupSpaceRemoteSpace[],
  extraIds: Array<string | undefined>
): GroupSpaceRemoteSpace[] {
  const byId = new Map<string, GroupSpaceRemoteSpace>();
  for (const item of spaces) {
    const remoteId = String(item.remote_space_id || "").trim();
    if (!remoteId) continue;
    byId.set(remoteId, item);
  }
  for (const rawId of extraIds) {
    const remoteId = String(rawId || "").trim();
    if (!remoteId || byId.has(remoteId)) continue;
    byId.set(remoteId, { remote_space_id: remoteId, title: "", created_at: "", is_owner: false });
  }
  return Array.from(byId.values());
}

function hasLaneTarget(binding: GroupSpaceBinding | null | undefined): boolean {
  const status = String(binding?.status || "").trim();
  const remoteId = String(binding?.remote_space_id || "").trim();
  return status === "bound" && Boolean(remoteId);
}

function laneVisualState(
  binding: GroupSpaceBinding | null | undefined,
  { providerUsable }: { providerUsable: boolean }
): LaneVisualState {
  if (!hasLaneTarget(binding)) return "unbound";
  return providerUsable ? "active" : "saved";
}

function laneStatusText(state: LaneVisualState, t: (key: string) => string): string {
  if (state === "active") return t("groupSpace.bound");
  if (state === "saved") return t("groupSpace.savedTarget");
  return t("groupSpace.notBound");
}

function statusChipClass(tone: "neutral" | "good" | "warn" | "danger"): string {
  if (tone === "good") {
    return "border-emerald-500/30 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300";
  }
  if (tone === "warn") {
    return "border-amber-500/30 bg-amber-500/15 text-amber-700 dark:text-amber-300";
  }
  if (tone === "danger") {
    return "border-rose-500/30 bg-rose-500/15 text-rose-700 dark:text-rose-300";
  }
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

export function GroupSpaceTab({ isDark: _isDark, groupId, isActive = true }: GroupSpaceTabProps) {
  const { t } = useTranslation("settings");
  const [provider] = useState("notebooklm");
  const [status, setStatus] = useState<GroupSpaceStatus | null>(null);
  const [authFlow, setAuthFlow] = useState<GroupSpaceProviderAuthStatus | null>(null);
  const [spaces, setSpaces] = useState<GroupSpaceRemoteSpace[]>([]);
  const [workBindRemoteId, setWorkBindRemoteId] = useState("");
  const [memoryBindRemoteId, setMemoryBindRemoteId] = useState("");
  const [loading, setLoading] = useState(false);
  const [spacesBusy, setSpacesBusy] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [hint, setHint] = useState("");
  const [err, setErr] = useState("");
  const connectHintedFlowRef = useRef("");
  const loadSeqRef = useRef(0);
  const pollInFlightRef = useRef(false);

  const providerState = status?.provider || null;
  const workBinding = status?.bindings?.work || null;
  const memoryBinding = status?.bindings?.memory || null;
  const workBoundRemoteId = String(workBinding?.remote_space_id || "").trim();
  const memoryBoundRemoteId = String(memoryBinding?.remote_space_id || "").trim();
  const authConfigured = Boolean(providerState?.auth_configured);
  const writeReady = Boolean(providerState?.write_ready);
  const connectionState = String(authFlow?.state || "idle").trim() || "idle";
  const connectionRunning = connectionState === "running";
  const connectionConnected = authConfigured;
  const providerUsable = connectionConnected && writeReady;
  const connectionWarning =
    String(authFlow?.error?.message || "").trim() || String(providerState?.last_error || "").trim();

  const notebookOptions = useMemo(
    () => mergeNotebookOptions(spaces, [workBoundRemoteId, memoryBoundRemoteId]),
    [spaces, workBoundRemoteId, memoryBoundRemoteId]
  );

  const workBoundNotebook = notebookOptions.find((item) => String(item.remote_space_id || "").trim() === workBoundRemoteId) || null;
  const memoryBoundNotebook = notebookOptions.find((item) => String(item.remote_space_id || "").trim() === memoryBoundRemoteId) || null;
  const workLaneState = laneVisualState(workBinding, { providerUsable });
  const memoryLaneState = laneVisualState(memoryBinding, { providerUsable });

  const connectionStatusText = useMemo(() => {
    if (connectionRunning) return t("groupSpace.accountConnecting");
    if (connectionConnected) return t("groupSpace.accountConnected");
    if (connectionState === "failed") return t("groupSpace.accountReconnectRequired");
    return t("groupSpace.accountNotConnected");
  }, [connectionConnected, connectionRunning, connectionState, t]);

  const connectionStatusTone = useMemo(() => {
    if (connectionRunning) return "text-[rgb(35,36,37)] dark:text-white";
    if (connectionConnected) return "text-emerald-600 dark:text-emerald-400";
    if (connectionState === "failed") return "text-amber-600 dark:text-amber-400";
    return "text-[var(--color-text-secondary)]";
  }, [connectionConnected, connectionRunning, connectionState]);

  const utilityButtonClass = secondaryButtonClass();
  const primaryUtilityButtonClass = primaryButtonClass(false);
  const dangerUtilityButtonClass =
    "inline-flex items-center justify-center gap-2 rounded-xl border border-rose-500/25 bg-rose-500/10 px-3.5 py-2.5 text-sm font-medium min-h-[44px] text-rose-700 dark:text-rose-300 hover:bg-rose-500/16 disabled:opacity-50 disabled:cursor-not-allowed";
  const compactDangerButtonClass =
    "inline-flex items-center justify-center gap-1.5 rounded-lg border border-rose-500/25 bg-rose-500/10 px-3 py-2 text-xs font-medium text-rose-700 dark:text-rose-300 hover:bg-rose-500/16 disabled:opacity-50 disabled:cursor-not-allowed";
  const showNotebookSection = connectionConnected;

  const setHintWithTimeout = (text: string) => {
    setHint(text);
    window.setTimeout(() => setHint(""), 2400);
  };

  const loadAll = async (opts?: { refreshSpaces?: boolean }) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const loadSeq = loadSeqRef.current + 1;
    loadSeqRef.current = loadSeq;
    const currentSpaces = spaces;
    setLoading(true);
    setErr("");
    try {
      let nextStatus: GroupSpaceStatus | null = null;
      let nextSpaces: GroupSpaceRemoteSpace[] = currentSpaces;
      const [statusResp, authResp] = await Promise.all([
        api.fetchGroupSpaceStatus(gid, provider),
        api.controlGroupSpaceProviderAuth({ provider, action: "status" }),
      ]);
      if (loadSeqRef.current !== loadSeq) return;

      if (!statusResp.ok) {
        setErr(statusResp.error?.message || t("groupSpace.loadFailed"));
      } else {
        nextStatus = statusResp.result || null;
        setStatus(nextStatus);
      }

      if (authResp.ok) {
        setAuthFlow(authResp.result?.auth || null);
      } else {
        setAuthFlow(null);
      }

      const writeReady = Boolean(nextStatus?.provider?.write_ready);
      const shouldLoadSpaces = shouldRefreshNotebookSpaces(
        writeReady,
        Boolean(opts?.refreshSpaces),
        currentSpaces.length,
      );
      if (shouldLoadSpaces) {
        setSpacesBusy(true);
        try {
          const spacesResp = await api.fetchGroupSpaceSpaces(gid, provider);
          if (loadSeqRef.current !== loadSeq) return;
          if (spacesResp.ok) {
            nextSpaces = normalizeNotebookSpaces(spacesResp.result?.spaces);
          }
        } finally {
          setSpacesBusy(false);
        }
      }
      nextSpaces = resolveNotebookSpacesAfterLoad(currentSpaces, {
        writeReady,
        fetchedSpaces: shouldLoadSpaces ? nextSpaces : null,
      });
      setSpaces(nextSpaces);

      const nextWorkBoundRemoteId = String(nextStatus?.bindings?.work?.remote_space_id || "").trim();
      const nextMemoryBoundRemoteId = String(nextStatus?.bindings?.memory?.remote_space_id || "").trim();
      setWorkBindRemoteId((prev) => resolveDraftNotebookId(prev, nextWorkBoundRemoteId, nextSpaces));
      setMemoryBindRemoteId((prev) => resolveDraftNotebookId(prev, nextMemoryBoundRemoteId, nextSpaces));
    } catch (e) {
      if (loadSeqRef.current !== loadSeq) return;
      setErr(e instanceof Error ? e.message : t("groupSpace.loadFailed"));
    } finally {
      if (loadSeqRef.current === loadSeq) {
        setLoading(false);
      }
    }
  };

  useEffect(() => {
    if (!isActive || !groupId) return;
    void loadAll({ refreshSpaces: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- refresh when active/group changes
  }, [isActive, groupId]);

  useEffect(() => {
    if (!isActive || !groupId || !connectionRunning) return;
    const pollOnce = async () => {
      if (pollInFlightRef.current) return;
      pollInFlightRef.current = true;
      try {
        const resp = await api.controlGroupSpaceProviderAuth({ provider, action: "status" });
        if (!resp.ok) {
          setErr(resp.error?.message || t("groupSpace.loadFailed"));
          return;
        }
        const nextAuth = resp.result?.auth || null;
        setAuthFlow(nextAuth);
        const nextState = String(nextAuth?.state || "").trim();
        if (nextState && nextState !== "running") {
          await loadAll({ refreshSpaces: nextState === "succeeded" });
          if (nextState === "succeeded") {
            const flowKey = String(nextAuth?.session_id || nextAuth?.started_at || "succeeded").trim();
            if (flowKey && connectHintedFlowRef.current !== flowKey) {
              connectHintedFlowRef.current = flowKey;
              setHintWithTimeout(t("groupSpace.googleConnectSuccess"));
            }
          }
        }
      } catch (e) {
        setErr(e instanceof Error ? e.message : t("groupSpace.loadFailed"));
      } finally {
        pollInFlightRef.current = false;
      }
    };
    void pollOnce();
    const timer = window.setInterval(() => {
      void pollOnce();
    }, 3000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- poll only while auth flow is running
  }, [isActive, groupId, connectionRunning]);

  const handleStartConnect = async (forceReauth: boolean = false) => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.controlGroupSpaceProviderAuth({
        provider,
        action: "start",
        timeoutSeconds: 900,
        forceReauth,
        projected: true,
      });
      if (!resp.ok) {
        setErr(
          resp.error?.message ||
            t(forceReauth ? "groupSpace.switchStartFailed" : "groupSpace.connectStartFailed")
        );
        return;
      }
      setAuthFlow(resp.result?.auth || null);
      setHintWithTimeout(t(forceReauth ? "groupSpace.switchStarted" : "groupSpace.connectStarted"));
    } catch {
      setErr(t(forceReauth ? "groupSpace.switchStartFailed" : "groupSpace.connectStartFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleCancelConnect = async () => {
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.controlGroupSpaceProviderAuth({ provider, action: "cancel" });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.connectCancelFailed"));
        return;
      }
      setAuthFlow(resp.result?.auth || null);
      setHintWithTimeout(t("groupSpace.connectCancelRequested"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.connectCancelFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleDisconnect = async () => {
    const confirmText = t("groupSpace.disconnectConfirm");
    if (!window.confirm(confirmText)) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.controlGroupSpaceProviderAuth({ provider, action: "disconnect" });
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.disconnectFailed"));
        return;
      }
      setAuthFlow(resp.result?.auth || null);
      setHintWithTimeout(t("groupSpace.disconnectSuccess"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.disconnectFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleBind = async (lane: NotebookLane, remoteSpaceId: string) => {
    const gid = String(groupId || "").trim();
    const rid = String(remoteSpaceId || "").trim();
    if (!gid || !rid) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.bindGroupSpace(gid, rid, provider, lane);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.bindFailed"));
        return;
      }
      setStatus(resp.result || null);
      if (lane === "work") setWorkBindRemoteId(rid);
      else setMemoryBindRemoteId(rid);
      setHintWithTimeout(t("groupSpace.bindSuccess"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.bindFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleCreateAndBind = async (lane: NotebookLane) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.bindGroupSpace(gid, "", provider, lane);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.bindFailed"));
        return;
      }
      const nextRemoteId = String(resp.result?.bindings?.[lane]?.remote_space_id || "").trim();
      setStatus(resp.result || null);
      if (lane === "work") setWorkBindRemoteId(nextRemoteId);
      else setMemoryBindRemoteId(nextRemoteId);
      setHintWithTimeout(t("groupSpace.bindCreated"));
      await loadAll({ refreshSpaces: true });
    } catch {
      setErr(t("groupSpace.bindFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const handleUnbind = async (lane: NotebookLane) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const confirmText = t(lane === "work" ? "groupSpace.workUnbindConfirm" : "groupSpace.memoryUnbindConfirm");
    if (!window.confirm(confirmText)) return;
    setActionBusy(true);
    setErr("");
    try {
      const resp = await api.unbindGroupSpace(gid, provider, lane);
      if (!resp.ok) {
        setErr(resp.error?.message || t("groupSpace.unbindFailed"));
        return;
      }
      setStatus(resp.result || null);
      if (lane === "work") setWorkBindRemoteId("");
      else setMemoryBindRemoteId("");
      setHintWithTimeout(t("groupSpace.unbindSuccess"));
      await loadAll();
    } catch {
      setErr(t("groupSpace.unbindFailed"));
    } finally {
      setActionBusy(false);
    }
  };

  const renderNotebookCard = (lane: NotebookLane) => {
    const isWork = lane === "work";
    const binding = (isWork ? workBinding : memoryBinding) || null;
    const boundRemoteId = isWork ? workBoundRemoteId : memoryBoundRemoteId;
    const selectedRemoteId = isWork ? workBindRemoteId : memoryBindRemoteId;
    const setSelectedRemoteId = isWork ? setWorkBindRemoteId : setMemoryBindRemoteId;
    const boundNotebook = isWork ? workBoundNotebook : memoryBoundNotebook;
    const titleKey = isWork ? "groupSpace.workNotebookTitle" : "groupSpace.memoryNotebookTitle";
    const hintKey = isWork ? "groupSpace.workNotebookHint" : "groupSpace.memoryNotebookHint";
    const laneState = laneVisualState(binding, { providerUsable });
    const notebookStateText = laneStatusText(laneState, t);
    const canBind = providerUsable && !actionBusy;
    const canUnbind = !actionBusy && Boolean(boundRemoteId);
    const laneTitleLabel = laneState === "saved" ? t("groupSpace.savedNotebookTarget") : t("groupSpace.currentNotebook");
    const boundNotebookTitle = String(boundNotebook?.title || "").trim();
    const showRemoteIdLine = Boolean(boundRemoteId && boundNotebookTitle && boundNotebookTitle !== boundRemoteId);
    const notebookActionsHint =
      laneState === "saved"
        ? t("groupSpace.savedTargetHint")
        : !connectionConnected
          ? t("groupSpace.connectGoogleFirst")
          : !writeReady
            ? (String(providerState?.last_error || "").trim() || t("groupSpace.adapterNotReady"))
            : "";
    const selectedNotebook =
      notebookOptions.find((item) => String(item.remote_space_id || "").trim() === String(selectedRemoteId || "").trim()) || null;
    const headerTone: "good" | "neutral" | "warn" = laneState === "active" ? "good" : laneState === "saved" ? "warn" : "neutral";

    return (
      <div key={lane} className={settingsWorkspacePanelClass(_isDark)}>
        <div>
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t(titleKey)}</div>
          <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">{t(hintKey)}</div>
        </div>

        <div className={`mt-3 ${settingsWorkspaceSoftPanelClass(_isDark)}`}>
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[11px] uppercase tracking-[0.18em] text-[var(--color-text-muted)]">{laneTitleLabel}</div>
              <div className="mt-1 text-sm font-semibold break-all text-[var(--color-text-primary)]">
                {boundRemoteId
                  ? (boundNotebookTitle || boundRemoteId)
                  : t("groupSpace.notBound")}
              </div>
              {showRemoteIdLine ? (
                <div className="mt-1 text-[11px] font-mono break-all text-[var(--color-text-muted)]">{boundRemoteId}</div>
              ) : null}
            </div>
            <div className={`shrink-0 rounded-full border px-2.5 py-1 text-[11px] font-medium ${statusChipClass(headerTone)}`}>
              {notebookStateText}
            </div>
          </div>
          {canUnbind ? (
            <div className="mt-3 flex justify-end">
              <button
                type="button"
                onClick={() => void handleUnbind(lane)}
                disabled={!canUnbind}
                className={compactDangerButtonClass}
              >
                <TrashIcon size={14} />
                {t("groupSpace.unbind")}
              </button>
            </div>
          ) : null}
        </div>

        <div className="mt-3">
          <label className="block text-[11px] mb-1 text-[var(--color-text-tertiary)]">{t("groupSpace.chooseNotebook")}</label>
          <SelectCombobox
            items={
              notebookOptions.length
                ? notebookOptions.map((item) => {
                    const remoteId = String(item.remote_space_id || "").trim();
                    const title = String(item.title || "").trim() || remoteId;
                    return { value: remoteId, label: title };
                  })
                : [{ value: "", label: t("groupSpace.noNotebookOptions"), disabled: true }]
            }
            value={selectedRemoteId}
            onChange={(value) => setSelectedRemoteId(String(value || ""))}
            disabled={!connectionConnected || actionBusy || spacesBusy || !notebookOptions.length}
            ariaLabel={t("groupSpace.chooseNotebook")}
            className={inputClass()}
            searchable
          />
          {notebookActionsHint ? (
            <div className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">{notebookActionsHint}</div>
          ) : connectionConnected && !notebookOptions.length ? (
            <div className="mt-1 text-[11px] text-[var(--color-text-tertiary)]">{t("groupSpace.noNotebookOptionsHint")}</div>
          ) : null}
        </div>

        <div className="mt-3 flex flex-col gap-2 sm:flex-row">
          <button
            type="button"
            onClick={() => void handleBind(lane, selectedRemoteId)}
            disabled={!canBind || !String(selectedRemoteId || "").trim()}
            className={`${primaryUtilityButtonClass} w-full sm:flex-1`}
          >
            <FolderIcon size={16} />
            {t("groupSpace.bindSelected")}
          </button>
          <button
            type="button"
            onClick={() => void handleCreateAndBind(lane)}
            disabled={!canBind}
            className={`${utilityButtonClass} w-full sm:w-auto sm:min-w-[132px]`}
          >
            <PlusIcon size={16} />
            {t("groupSpace.createAndBind")}
          </button>
        </div>
        {selectedNotebook ? (
          <div className="mt-2 text-[11px] text-[var(--color-text-tertiary)]">
            {t("groupSpace.bindSelectedHintWithTarget", {
              target: String(selectedNotebook.title || "").trim() || String(selectedNotebook.remote_space_id || "").trim(),
            })}
          </div>
        ) : null}
      </div>
    );
  };

  if (!groupId) {
    return <div className="text-sm text-[var(--color-text-tertiary)]">{t("groupSpace.openFromGroup")}</div>;
  }

  return (
    <div className="space-y-5">
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("groupSpace.title")}</h3>
            <p className="mt-1 max-w-3xl text-xs text-[var(--color-text-muted)]">{t("groupSpace.description")}</p>
          </div>
          <button
            type="button"
            onClick={() => void loadAll()}
            disabled={actionBusy || loading}
            className={secondaryButtonClass("sm")}
          >
            <RefreshIcon size={16} />
            {loading ? t("common:loading") : t("groupSpace.refresh")}
          </button>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspacePanelClass(_isDark)}>
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("groupSpace.accountTitle")}</div>
                <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">{t("groupSpace.accountHint")}</div>
              </div>
              <div className={`text-xs font-medium ${connectionStatusTone}`}>{connectionStatusText}</div>
            </div>

            <div className="mt-3 flex flex-wrap gap-2">
              {connectionRunning ? (
                <button
                  type="button"
                  onClick={() => void handleCancelConnect()}
                  disabled={actionBusy}
                  className={dangerUtilityButtonClass}
                >
                  <CloseIcon size={16} />
                  {t("groupSpace.cancelConnect")}
                </button>
              ) : !connectionConnected ? (
                <button
                  type="button"
                  onClick={() => void handleStartConnect(false)}
                  disabled={actionBusy}
                  className={primaryUtilityButtonClass}
                >
                  <GlobeIcon size={16} />
                  {t("groupSpace.connectGoogle")}
                </button>
              ) : (
                <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-2">
                  <button
                    type="button"
                    onClick={() => void handleStartConnect(true)}
                    disabled={actionBusy}
                    className={utilityButtonClass}
                  >
                    <RefreshIcon size={16} />
                    {t("groupSpace.switchGoogle")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleDisconnect()}
                    disabled={actionBusy}
                    className={dangerUtilityButtonClass}
                  >
                    <PowerIcon size={16} />
                    {t("groupSpace.disconnectGoogle")}
                  </button>
                </div>
              )}
            </div>

            {connectionConnected && !connectionRunning ? (
              <div className="mt-3 text-xs text-[var(--color-text-tertiary)]">
                {t("groupSpace.disconnectBehaviorHint")}
              </div>
            ) : null}

            {authFlow?.message ? (
              <div className="mt-3 text-xs text-[var(--color-text-tertiary)]">{String(authFlow.message)}</div>
            ) : null}
            {authFlow?.error?.message ? (
              <div className="mt-2 text-xs text-amber-600 dark:text-amber-400">{String(authFlow.error.message)}</div>
            ) : null}
            {connectionRunning &&
            String(authFlow?.delivery || "").trim() === "projected_browser" &&
            Boolean(authFlow?.projected_browser?.active) ? (
              <div className="mt-4">
                <ProjectedBrowserSurfacePanel
                  key={String(authFlow?.session_id || authFlow?.started_at || "notebooklm-auth")}
                  isDark={_isDark}
                  refreshNonce={0}
                  viewportClassName="h-[68vh] min-h-[460px] max-h-[780px] w-full sm:h-[72vh] sm:min-h-[560px]"
                  fallbackUrl="https://notebooklm.google.com/"
                  labels={{
                    starting: t("groupSpace.projectedAuthStarting"),
                    waiting: t("groupSpace.projectedAuthWaiting"),
                    ready: t("groupSpace.projectedAuthReady"),
                    failed: t("groupSpace.projectedAuthFailed"),
                    closed: t("groupSpace.projectedAuthClosed"),
                    reconnecting: t("groupSpace.projectedAuthReconnecting"),
                    reconnect: t("groupSpace.projectedAuthReconnect"),
                    back: t("groupSpace.projectedAuthBack"),
                    frameAlt: t("groupSpace.projectedAuthFrameAlt"),
                  }}
                  webSocketUrl={api.getGroupSpaceProviderAuthBrowserWebSocketUrl(provider)}
                  loadSession={() => api.fetchGroupSpaceProviderAuthBrowserSession(provider)}
                />
              </div>
            ) : null}
          </div>

          {showNotebookSection ? (
            <>
              <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
                {NOTEBOOK_LANES.map((lane) => renderNotebookCard(lane))}
              </div>

              <div className={settingsWorkspacePanelClass(_isDark)}>
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("groupSpace.summaryTitle")}</div>
                <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-3">
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">{t("groupSpace.summaryGoogle")}</div>
                    <div className={`mt-1 text-sm font-medium ${connectionStatusTone}`}>{connectionStatusText}</div>
                  </div>
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">{t("groupSpace.summaryWork")}</div>
                    <div className={`mt-1 text-sm font-medium ${workLaneState === "active" ? "text-emerald-600 dark:text-emerald-400" : workLaneState === "saved" ? "text-amber-600 dark:text-amber-400" : "text-[var(--color-text-primary)]"}`}>
                      {laneStatusText(workLaneState, t)}
                    </div>
                  </div>
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">{t("groupSpace.summaryMemory")}</div>
                    <div className={`mt-1 text-sm font-medium ${memoryLaneState === "active" ? "text-emerald-600 dark:text-emerald-400" : memoryLaneState === "saved" ? "text-amber-600 dark:text-amber-400" : "text-[var(--color-text-primary)]"}`}>
                      {laneStatusText(memoryLaneState, t)}
                    </div>
                  </div>
                </div>
                {connectionWarning ? (
                  <div className="mt-3 text-xs text-amber-600 dark:text-amber-400">
                    {t("groupSpace.summaryWarning")}: {connectionWarning}
                  </div>
                ) : null}
              </div>
            </>
          ) : (
            <div className={settingsWorkspacePanelClass(_isDark)}>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("groupSpace.notebookSectionLockedTitle")}</div>
              <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">
                {connectionRunning
                  ? t("groupSpace.notebookSectionLockedConnecting")
                  : t("groupSpace.notebookSectionLockedDisconnected")}
              </div>
            </div>
          )}

          {err ? <div className="text-xs text-rose-600 dark:text-rose-400">{err}</div> : null}
          {hint ? <div className="text-xs text-emerald-600 dark:text-emerald-400">{hint}</div> : null}
        </div>
      </section>
    </div>
  );
}
