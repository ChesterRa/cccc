import { useEffect, useRef, useState } from "react";
import type { TFunction } from "i18next";
import type { Actor } from "../../../types";
import * as api from "../../../services/api";
import { useObservabilityStore } from "../../../stores";
import { copyTextToClipboard } from "../../../utils/copy";
import { loadedWebEntry, type RuntimeBuildInfo } from "../../../utils/runtimeBuildInfo";
import {
  DEFAULT_ASSISTANT_RUNTIME_VISIBILITY,
  DEFAULT_PEER_RUNTIME_VISIBILITY,
  normalizeRuntimeVisibilityMode,
  type RuntimeVisibilityMode,
} from "../../../utils/runtimeVisibility";
import type { GlobalTabId, GroupTabId, SettingsScope } from "./types";

export function useSettingsDiagnostics(args: {
  isOpen: boolean;
  groupId?: string;
  scope: SettingsScope;
  groupTab: GroupTabId;
  globalTab: GlobalTabId;
  hasSettingsTarget: boolean;
  canAccessGlobalSettings: boolean | null;
  onRegistryChanged?: () => Promise<void> | void;
  t: TFunction<"settings">;
}) {
  const {
    isOpen,
    groupId,
    scope,
    groupTab,
    globalTab,
    hasSettingsTarget,
    canAccessGlobalSettings,
    onRegistryChanged,
    t,
  } = args;

  // Terminal transcript tail viewer
  const [tailActorId, setTailActorId] = useState("");
  const [tailMaxChars, setTailMaxChars] = useState(8000);
  const [tailStripAnsi, setTailStripAnsi] = useState(true);
  const [tailCompact, setTailCompact] = useState(true);
  const [tailText, setTailText] = useState("");
  const [tailHint, setTailHint] = useState("");
  const [tailErr, setTailErr] = useState("");
  const [tailBusy, setTailBusy] = useState(false);
  const [tailCopyInfo, setTailCopyInfo] = useState("");

  // Global observability (developer mode)
  const [developerMode, setDeveloperMode] = useState(false);
  const [logLevel, setLogLevel] = useState<"INFO" | "DEBUG">("INFO");
  const [terminalBacklogMiB, setTerminalBacklogMiB] = useState(10);
  const [terminalScrollbackLines, setTerminalScrollbackLines] = useState(8000);
  const [peerRuntimeVisibility, setPeerRuntimeVisibility] =
    useState<RuntimeVisibilityMode>("visible");
  const [assistantRuntimeVisibility, setAssistantRuntimeVisibility] =
    useState<RuntimeVisibilityMode>("hidden");
  const [obsBusy, setObsBusy] = useState(false);
  const observabilityLoaded = useRef(false);

  // Developer-mode debug views
  const [devActors, setDevActors] = useState<Actor[]>([]);
  const [debugSnapshot, setDebugSnapshot] = useState("");
  const [debugSnapshotErr, setDebugSnapshotErr] = useState("");
  const [debugSnapshotBusy, setDebugSnapshotBusy] = useState(false);
  const [runtimeVersion, setRuntimeVersion] = useState("");
  const [runtimeBuildInfo, setRuntimeBuildInfo] = useState<RuntimeBuildInfo>();
  const [daemonVersion, setDaemonVersion] = useState("");
  const [runtimeInfoErr, setRuntimeInfoErr] = useState("");

  const [logComponent, setLogComponent] = useState<"daemon" | "web" | "im">("daemon");
  const [logLines, setLogLines] = useState(200);
  const [logText, setLogText] = useState("");
  const [logErr, setLogErr] = useState("");
  const [logBusy, setLogBusy] = useState(false);

  // Registry maintenance (global)
  const [registryBusy, setRegistryBusy] = useState(false);
  const [registryErr, setRegistryErr] = useState("");
  const [registryResult, setRegistryResult] = useState<api.RegistryReconcileResult | null>(null);

  // Reads and clears share one sequence per output so only the latest action
  // may update that output or end its busy state. Cleanup retires the visit.
  const tailRequest = useRef(0);
  const snapshotRequest = useRef(0);
  const logRequest = useRef(0);
  useEffect(() => {
    setTailText("");
    setTailHint("");
    setTailErr("");
    setTailCopyInfo("");
    setTailBusy(false);
    return () => {
      tailRequest.current += 1;
    };
  }, [groupId, tailActorId, isOpen]);
  useEffect(() => {
    setDebugSnapshot("");
    setDebugSnapshotErr("");
    setDebugSnapshotBusy(false);
    return () => {
      snapshotRequest.current += 1;
    };
  }, [groupId, isOpen]);
  useEffect(() => {
    setLogText("");
    setLogErr("");
    setLogBusy(false);
    return () => {
      logRequest.current += 1;
    };
  }, [groupId, logComponent, isOpen]);

  const developerTabActive = !hasSettingsTarget && scope === "global" && globalTab === "developer";

  useEffect(() => {
    setDevActors([]);
    setTailActorId("");
  }, [groupId]);

  useEffect(() => {
    if (!isOpen) {
      observabilityLoaded.current = false;
    }
  }, [isOpen]);

  const loadObservability = async () => {
    try {
      const resp = await api.fetchObservability();
      if (resp.ok && resp.result?.observability) {
        const obs = resp.result.observability;
        useObservabilityStore.getState().setFromObs(obs);
        setDeveloperMode(Boolean(obs.developer_mode));
        const lvl = String(obs.log_level || "INFO").toUpperCase();
        setLogLevel(lvl === "DEBUG" ? "DEBUG" : "INFO");
        const perActorBytes = Number(obs.terminal_transcript?.per_actor_bytes || 0);
        if (Number.isFinite(perActorBytes) && perActorBytes > 0) {
          setTerminalBacklogMiB(Math.max(1, Math.round(perActorBytes / (1024 * 1024))));
        }
        const scrollbackLines = Number(obs.terminal_ui?.scrollback_lines || 0);
        if (Number.isFinite(scrollbackLines) && scrollbackLines > 0) {
          setTerminalScrollbackLines(Math.max(1000, Math.round(scrollbackLines)));
        }
        setPeerRuntimeVisibility(
          normalizeRuntimeVisibilityMode(
            obs.runtime_visibility?.peer_runtime,
            DEFAULT_PEER_RUNTIME_VISIBILITY,
          ),
        );
        setAssistantRuntimeVisibility(
          normalizeRuntimeVisibilityMode(
            obs.runtime_visibility?.assistant_runtime,
            DEFAULT_ASSISTANT_RUNTIME_VISIBILITY,
          ),
        );
      }
    } catch (e) {
      console.error("Failed to load observability settings:", e);
    }
  };

  const loadRuntimeInfo = async () => {
    setRuntimeInfoErr("");
    setRuntimeBuildInfo(undefined);
    try {
      const resp = await api.fetchPing();
      if (!resp.ok) {
        setRuntimeVersion("");
        setDaemonVersion("");
        setRuntimeInfoErr(resp.error?.message || "Failed to load runtime info");
        return;
      }
      const result = resp.result || {};
      const daemon =
        result.daemon && typeof result.daemon === "object" && !Array.isArray(result.daemon)
          ? (result.daemon as Record<string, unknown>)
          : null;
      setRuntimeVersion(String(result.version || "").trim());
      setDaemonVersion(String(daemon?.version || "").trim());
      setRuntimeBuildInfo({
        webSource: String(result.build?.source_id || ""),
        daemonSource: String(
          (daemon?.build as { source_id?: string } | undefined)?.source_id || "",
        ),
        webAssets: String(result.web?.assets_id || ""),
        servedEntry: String(result.web?.entry_script || ""),
        loadedEntry: loadedWebEntry(),
      });
    } catch {
      setRuntimeVersion("");
      setDaemonVersion("");
      setRuntimeInfoErr("Failed to load runtime info");
    }
  };

  const loadLogTail = async () => {
    if (logComponent === "im" && !groupId) {
      setLogText("");
      setLogErr(t("developer.imLogsRequireGroup"));
      return;
    }
    const request = ++logRequest.current;
    setLogBusy(true);
    setLogErr("");
    try {
      const resp = await api.fetchLogTail(logComponent, groupId || "", logLines || 200);
      if (request !== logRequest.current) return;
      if (resp.ok) {
        const lines = Array.isArray(resp.result?.lines) ? resp.result.lines : [];
        setLogText(lines.join("\n"));
      } else {
        setLogText("");
        setLogErr(resp.error?.message || "Failed to tail logs");
      }
    } catch {
      if (request === logRequest.current) {
        setLogText("");
        setLogErr("Failed to tail logs");
      }
    } finally {
      if (request === logRequest.current) setLogBusy(false);
    }
  };

  const loadRegistryPreview = async () => {
    setRegistryBusy(true);
    setRegistryErr("");
    try {
      const resp = await api.previewRegistryReconcile();
      if (resp.ok) {
        setRegistryResult(resp.result);
      } else {
        setRegistryErr(resp.error?.message || "Failed to scan registry");
      }
    } catch {
      setRegistryErr("Failed to scan registry");
    } finally {
      setRegistryBusy(false);
    }
  };

  useEffect(() => {
    if (
      !isOpen ||
      !developerTabActive ||
      canAccessGlobalSettings !== true ||
      observabilityLoaded.current
    )
      return;
    observabilityLoaded.current = true;
    void loadObservability();
  }, [isOpen, developerTabActive, canAccessGlobalSettings]);

  useEffect(() => {
    if (
      !isOpen ||
      !groupId ||
      hasSettingsTarget ||
      scope !== "group" ||
      (groupTab !== "automation" && groupTab !== "transcript")
    )
      return;
    let cancelled = false;
    void api
      .fetchActors(groupId, false)
      .then((resp) => {
        if (cancelled || !resp.ok) return;
        const actors = Array.isArray(resp.result?.actors) ? resp.result.actors : [];
        setDevActors(actors);
        setTailActorId((current) =>
          actors.some((actor) => actor.id === current) ? current : actors[0]?.id || "",
        );
      })
      .catch((error) => {
        if (!cancelled) console.error("Failed to load settings actor list:", error);
      });
    return () => {
      cancelled = true;
    };
  }, [isOpen, groupId, hasSettingsTarget, scope, groupTab]);

  useEffect(() => {
    if (!isOpen) return;
    if (scope !== "global" || globalTab !== "developer") return;
    void loadRegistryPreview();
  }, [isOpen, scope, globalTab]);

  useEffect(() => {
    if (!isOpen) return;
    if (scope !== "global" || globalTab !== "developer") return;
    void loadRuntimeInfo();
  }, [isOpen, scope, globalTab]);

  useEffect(() => {
    if (!isOpen || !developerMode) return;
    if (scope !== "global" || globalTab !== "developer") return;
    void loadLogTail();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Keep refresh tied to modal/view controls; callback identity is not meaningful here.
  }, [developerMode, globalTab, groupId, isOpen, logComponent, logLines, scope]);

  const copyTailLastLines = async (lineCount: number) => {
    const n = Math.max(1, Math.min(200, Number(lineCount || 0) || 50));
    const text = String(tailText || "");
    if (!text.trim()) return;
    const lines = text.split("\n");
    const payload = lines
      .slice(Math.max(0, lines.length - n))
      .join("\n")
      .trimEnd();
    if (!payload) return;

    const setToast = (msg: string) => {
      setTailCopyInfo(msg);
      window.setTimeout(() => setTailCopyInfo(""), 1200);
    };

    const ok = await copyTextToClipboard(payload);
    setToast(ok ? t("automation.copiedLines", { n }) : t("common:copyFailed"));
  };

  const loadTerminalTail = async () => {
    if (!groupId || !tailActorId) return;
    const request = ++tailRequest.current;
    setTailBusy(true);
    setTailErr("");
    try {
      const resp = await api.fetchTerminalTail(
        groupId,
        tailActorId,
        tailMaxChars || 8000,
        tailStripAnsi,
        tailCompact,
      );
      if (request !== tailRequest.current) return;
      if (resp.ok) {
        setTailText(String(resp.result?.text || ""));
        setTailHint(String(resp.result?.hint || ""));
      } else {
        setTailText("");
        setTailHint("");
        setTailErr(resp.error?.message || t("automation.failedToLoadTranscript"));
      }
    } catch {
      if (request === tailRequest.current) {
        setTailText("");
        setTailHint("");
        setTailErr(t("automation.failedToLoadTranscript"));
      }
    } finally {
      if (request === tailRequest.current) setTailBusy(false);
    }
  };

  const clearTail = async () => {
    if (!groupId || !tailActorId) return;
    const request = ++tailRequest.current;
    setTailBusy(true);
    setTailErr("");
    try {
      const resp = await api.clearTerminalTail(groupId, tailActorId);
      if (request !== tailRequest.current) return;
      if (!resp.ok) {
        setTailErr(resp.error?.message || t("automation.failedToClearTranscript"));
        return;
      }
      setTailText("");
      setTailHint("");
    } catch {
      if (request === tailRequest.current) setTailErr(t("automation.failedToClearTranscript"));
    } finally {
      if (request === tailRequest.current) setTailBusy(false);
    }
  };

  const handleSaveObservability = async () => {
    setObsBusy(true);
    try {
      const perActorBytes =
        Math.max(1, Math.min(50, Number(terminalBacklogMiB || 0))) * 1024 * 1024;
      const scrollbackLines = Math.max(
        1000,
        Math.min(200000, Number(terminalScrollbackLines || 0)),
      );
      const resp = await api.updateObservability({
        developerMode,
        logLevel,
        terminalTranscriptPerActorBytes: perActorBytes,
        terminalUiScrollbackLines: scrollbackLines,
        peerRuntimeVisibility,
        assistantRuntimeVisibility,
      });
      if (resp.ok && resp.result?.observability) {
        const obs = resp.result.observability;
        useObservabilityStore.getState().setFromObs(obs);
        setDeveloperMode(Boolean(obs.developer_mode));
        const lvl = String(obs.log_level || "INFO").toUpperCase();
        setLogLevel(lvl === "DEBUG" ? "DEBUG" : "INFO");
        const bytes = Number(obs.terminal_transcript?.per_actor_bytes || 0);
        if (Number.isFinite(bytes) && bytes > 0) {
          setTerminalBacklogMiB(Math.max(1, Math.round(bytes / (1024 * 1024))));
        }
        const lines = Number(obs.terminal_ui?.scrollback_lines || 0);
        if (Number.isFinite(lines) && lines > 0) {
          setTerminalScrollbackLines(Math.max(1000, Math.round(lines)));
        }
        setPeerRuntimeVisibility(
          normalizeRuntimeVisibilityMode(
            obs.runtime_visibility?.peer_runtime,
            DEFAULT_PEER_RUNTIME_VISIBILITY,
          ),
        );
        setAssistantRuntimeVisibility(
          normalizeRuntimeVisibilityMode(
            obs.runtime_visibility?.assistant_runtime,
            DEFAULT_ASSISTANT_RUNTIME_VISIBILITY,
          ),
        );
      } else if (resp.ok) {
        await loadObservability();
      }
    } catch {
      // ignore
    } finally {
      setObsBusy(false);
    }
  };

  const loadDebugSnapshot = async () => {
    if (!groupId) return;
    const request = ++snapshotRequest.current;
    setDebugSnapshotBusy(true);
    setDebugSnapshotErr("");
    try {
      const resp = await api.fetchDebugSnapshot(groupId);
      if (request !== snapshotRequest.current) return;
      if (resp.ok) {
        setDebugSnapshot(JSON.stringify(resp.result ?? {}, null, 2));
      } else {
        setDebugSnapshot("");
        setDebugSnapshotErr(resp.error?.message || "Failed to load debug snapshot");
      }
    } catch {
      if (request === snapshotRequest.current) {
        setDebugSnapshot("");
        setDebugSnapshotErr("Failed to load debug snapshot");
      }
    } finally {
      if (request === snapshotRequest.current) setDebugSnapshotBusy(false);
    }
  };

  const clearDebugSnapshot = () => {
    snapshotRequest.current += 1;
    setDebugSnapshot("");
    setDebugSnapshotErr("");
    setDebugSnapshotBusy(false);
  };

  const handleClearLogs = async () => {
    if (!developerMode) return;
    if (logComponent === "im" && !groupId) {
      setLogErr(t("developer.imLogsRequireGroup"));
      return;
    }
    const request = ++logRequest.current;
    setLogBusy(true);
    setLogErr("");
    try {
      const resp = await api.clearLogs(logComponent, groupId || "");
      if (request !== logRequest.current) return;
      if (!resp.ok) {
        setLogErr(resp.error?.message || "Failed to clear logs");
        return;
      }
      setLogText("");
    } catch {
      if (request === logRequest.current) setLogErr("Failed to clear logs");
    } finally {
      if (request === logRequest.current) setLogBusy(false);
    }
  };

  const handleReconcileRegistry = async () => {
    const missingCount = registryResult?.missing_group_ids?.length || 0;
    if (missingCount <= 0) {
      await loadRegistryPreview();
      return;
    }
    if (!window.confirm(t("automation.removeRegistryConfirm", { count: missingCount }))) {
      return;
    }
    setRegistryBusy(true);
    setRegistryErr("");
    try {
      const resp = await api.executeRegistryReconcile(true);
      if (resp.ok) {
        setRegistryResult(resp.result);
        if (onRegistryChanged) {
          await onRegistryChanged();
        }
        await loadRegistryPreview();
      } else {
        setRegistryErr(resp.error?.message || "Failed to clean registry");
      }
    } catch {
      setRegistryErr("Failed to clean registry");
    } finally {
      setRegistryBusy(false);
    }
  };

  return {
    tailActorId,
    setTailActorId,
    tailMaxChars,
    setTailMaxChars,
    tailStripAnsi,
    setTailStripAnsi,
    tailCompact,
    setTailCompact,
    tailText,
    tailHint,
    tailErr,
    tailBusy,
    tailCopyInfo,
    copyTailLastLines,
    loadTerminalTail,
    clearTail,
    devActors,
    developerMode,
    setDeveloperMode,
    logLevel,
    setLogLevel,
    terminalBacklogMiB,
    setTerminalBacklogMiB,
    terminalScrollbackLines,
    setTerminalScrollbackLines,
    peerRuntimeVisibility,
    setPeerRuntimeVisibility,
    assistantRuntimeVisibility,
    setAssistantRuntimeVisibility,
    obsBusy,
    handleSaveObservability,
    debugSnapshot,
    debugSnapshotErr,
    debugSnapshotBusy,
    loadDebugSnapshot,
    clearDebugSnapshot,
    runtimeVersion,
    runtimeBuildInfo,
    daemonVersion,
    runtimeInfoErr,
    logComponent,
    setLogComponent,
    logLines,
    setLogLines,
    logText,
    logErr,
    logBusy,
    loadLogTail,
    handleClearLogs,
    registryBusy,
    registryErr,
    registryResult,
    loadRegistryPreview,
    handleReconcileRegistry,
  };
}
