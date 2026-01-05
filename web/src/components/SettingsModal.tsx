// SettingsModal - 设置弹窗主组件
import { useState, useEffect } from "react";
import { Actor, GroupSettings, IMStatus } from "../types";
import * as api from "../services/api";
import {
  TimingTab,
  IMBridgeTab,
  TranscriptTab,
  RemoteAccessTab,
  DeveloperTab,
  SettingsScope,
  GroupTabId,
  GlobalTabId,
} from "./modals/settings";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  settings: GroupSettings | null;
  onUpdateSettings: (settings: Partial<GroupSettings>) => Promise<void>;
  busy: boolean;
  isDark: boolean;
  groupId?: string;
}

export function SettingsModal({
  isOpen,
  onClose,
  settings,
  onUpdateSettings,
  busy,
  isDark,
  groupId,
}: SettingsModalProps) {
  const [scope, setScope] = useState<SettingsScope>(groupId ? "group" : "global");
  const [groupTab, setGroupTab] = useState<GroupTabId>("timing");
  const [globalTab, setGlobalTab] = useState<GlobalTabId>("remote");

  // Timing settings state
  const [nudgeSeconds, setNudgeSeconds] = useState(300);
  const [idleSeconds, setIdleSeconds] = useState(600);
  const [keepaliveSeconds, setKeepaliveSeconds] = useState(120);
  const [silenceSeconds, setSilenceSeconds] = useState(600);
  const [deliveryInterval, setDeliveryInterval] = useState(0);
  const [standupInterval, setStandupInterval] = useState(900);

  // Terminal transcript (group-scoped policy)
  const [terminalVisibility, setTerminalVisibility] = useState<"off" | "foreman" | "all">("foreman");
  const [terminalNotifyTail, setTerminalNotifyTail] = useState(false);
  const [terminalNotifyLines, setTerminalNotifyLines] = useState(20);

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

  // IM Bridge state
  const [imStatus, setImStatus] = useState<IMStatus | null>(null);
  const [imPlatform, setImPlatform] = useState<"telegram" | "slack" | "discord">("telegram");
  const [imBotTokenEnv, setImBotTokenEnv] = useState("");
  const [imAppTokenEnv, setImAppTokenEnv] = useState("");
  const [imBusy, setImBusy] = useState(false);

  // Global observability (developer mode)
  const [developerMode, setDeveloperMode] = useState(false);
  const [logLevel, setLogLevel] = useState<"INFO" | "DEBUG">("INFO");
  const [obsBusy, setObsBusy] = useState(false);

  // Developer-mode debug views
  const [devActors, setDevActors] = useState<Actor[]>([]);
  const [debugSnapshot, setDebugSnapshot] = useState("");
  const [debugSnapshotErr, setDebugSnapshotErr] = useState("");
  const [debugSnapshotBusy, setDebugSnapshotBusy] = useState(false);

  const [logComponent, setLogComponent] = useState<"daemon" | "web" | "im">("daemon");
  const [logLines, setLogLines] = useState(200);
  const [logText, setLogText] = useState("");
  const [logErr, setLogErr] = useState("");
  const [logBusy, setLogBusy] = useState(false);

  // ============ Effects ============

  useEffect(() => {
    if (isOpen && settings) {
      setNudgeSeconds(settings.nudge_after_seconds);
      setIdleSeconds(settings.actor_idle_timeout_seconds);
      setKeepaliveSeconds(settings.keepalive_delay_seconds);
      setSilenceSeconds(settings.silence_timeout_seconds);
      setDeliveryInterval(settings.min_interval_seconds);
      setStandupInterval(settings.standup_interval_seconds ?? 900);
      setTerminalVisibility(settings.terminal_transcript_visibility || "foreman");
      setTerminalNotifyTail(Boolean(settings.terminal_transcript_notify_tail));
      setTerminalNotifyLines(Number(settings.terminal_transcript_notify_lines || 20));
    }
  }, [isOpen, settings]);

  useEffect(() => {
    if (!isOpen) return;
    setScope(groupId ? "group" : "global");
  }, [isOpen, groupId]);

  useEffect(() => {
    if (isOpen && groupId) loadIMStatus();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在 modal 打开或 groupId 变化时加载
  }, [isOpen, groupId]);

  useEffect(() => {
    if (isOpen) loadObservability();
  }, [isOpen]);

  useEffect(() => {
    if (isOpen && groupId) loadDevActors();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在 modal 打开或 groupId 变化时加载
  }, [isOpen, groupId]);

  // ============ Data Loading ============

  const loadIMStatus = async () => {
    if (!groupId) return;
    try {
      const statusResp = await api.fetchIMStatus(groupId);
      if (statusResp.ok) {
        setImStatus(statusResp.result);
        if (statusResp.result.platform) {
          setImPlatform(statusResp.result.platform as "telegram" | "slack" | "discord");
        }
      }
      const configResp = await api.fetchIMConfig(groupId);
      if (configResp.ok && configResp.result.im) {
        const im = configResp.result.im;
        if (im.platform) setImPlatform(im.platform);
        setImBotTokenEnv(im.bot_token_env || im.token_env || im.token || "");
        setImAppTokenEnv(im.app_token_env || "");
      }
    } catch (e) {
      console.error("Failed to load IM status:", e);
    }
  };

  const loadObservability = async () => {
    try {
      const resp = await api.fetchObservability();
      if (resp.ok && resp.result?.observability) {
        const obs = resp.result.observability;
        setDeveloperMode(Boolean(obs.developer_mode));
        const lvl = String(obs.log_level || "INFO").toUpperCase();
        setLogLevel(lvl === "DEBUG" ? "DEBUG" : "INFO");
      }
    } catch {
      // ignore
    }
  };

  const loadDevActors = async () => {
    if (!groupId) return;
    try {
      const resp = await api.fetchActors(groupId);
      if (resp.ok && resp.result?.actors) {
        const actors = Array.isArray(resp.result.actors) ? resp.result.actors : [];
        setDevActors(actors);
        if (!tailActorId && actors.length > 0) {
          setTailActorId(actors[0].id);
        }
      }
    } catch {
      // ignore
    }
  };

  // ============ Handlers ============

  const handleSaveSettings = async () => {
    await onUpdateSettings({
      nudge_after_seconds: nudgeSeconds,
      actor_idle_timeout_seconds: idleSeconds,
      keepalive_delay_seconds: keepaliveSeconds,
      silence_timeout_seconds: silenceSeconds,
      min_interval_seconds: deliveryInterval,
      standup_interval_seconds: standupInterval,
    });
  };

  const handleSaveTranscriptSettings = async () => {
    await onUpdateSettings({
      terminal_transcript_visibility: terminalVisibility,
      terminal_transcript_notify_tail: terminalNotifyTail,
      terminal_transcript_notify_lines: terminalNotifyLines,
    });
  };

  const copyTailLastLines = async (lineCount: number) => {
    const n = Math.max(1, Math.min(200, Number(lineCount || 0) || 50));
    const text = String(tailText || "");
    if (!text.trim()) return;
    const lines = text.split("\n");
    const payload = lines.slice(Math.max(0, lines.length - n)).join("\n").trimEnd();
    if (!payload) return;

    const setToast = (msg: string) => {
      setTailCopyInfo(msg);
      window.setTimeout(() => setTailCopyInfo(""), 1200);
    };

    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
        await navigator.clipboard.writeText(payload);
        setToast(`Copied last ${n} lines`);
        return;
      }
    } catch {
      // fallback
    }

    try {
      const el = document.createElement("textarea");
      el.value = payload;
      el.style.position = "fixed";
      el.style.left = "-9999px";
      el.style.top = "0";
      document.body.appendChild(el);
      el.focus();
      el.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(el);
      setToast(ok ? `Copied last ${n} lines` : "Copy failed");
    } catch {
      setToast("Copy failed");
    }
  };

  const loadTerminalTail = async () => {
    if (!groupId || !tailActorId) return;
    setTailBusy(true);
    setTailErr("");
    try {
      const resp = await api.fetchTerminalTail(groupId, tailActorId, tailMaxChars || 8000, tailStripAnsi, tailCompact);
      if (resp.ok) {
        setTailText(String(resp.result?.text || ""));
        setTailHint(String(resp.result?.hint || ""));
      } else {
        setTailText("");
        setTailHint("");
        setTailErr(resp.error?.message || "Failed to load terminal transcript");
      }
    } catch {
      setTailText("");
      setTailHint("");
      setTailErr("Failed to load terminal transcript");
    } finally {
      setTailBusy(false);
    }
  };

  const clearTail = async () => {
    if (!groupId || !tailActorId) return;
    setTailBusy(true);
    setTailErr("");
    try {
      const resp = await api.clearTerminalTail(groupId, tailActorId);
      if (!resp.ok) {
        setTailErr(resp.error?.message || "Failed to clear terminal transcript");
        return;
      }
      setTailText("");
      setTailHint("");
    } catch {
      setTailErr("Failed to clear terminal transcript");
    } finally {
      setTailBusy(false);
    }
  };

  const handleSaveIMConfig = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      const resp = await api.setIMConfig(groupId, imPlatform, imBotTokenEnv, imAppTokenEnv);
      if (resp.ok) await loadIMStatus();
    } catch (e) {
      console.error("Failed to save IM config:", e);
    } finally {
      setImBusy(false);
    }
  };

  const handleRemoveIMConfig = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      const resp = await api.unsetIMConfig(groupId);
      if (resp.ok) {
        setImBotTokenEnv("");
        setImAppTokenEnv("");
        await loadIMStatus();
      }
    } catch (e) {
      console.error("Failed to remove IM config:", e);
    } finally {
      setImBusy(false);
    }
  };

  const handleStartBridge = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      await api.startIMBridge(groupId);
      await loadIMStatus();
    } catch (e) {
      console.error("Failed to start bridge:", e);
    } finally {
      setImBusy(false);
    }
  };

  const handleStopBridge = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      await api.stopIMBridge(groupId);
      await loadIMStatus();
    } catch (e) {
      console.error("Failed to stop bridge:", e);
    } finally {
      setImBusy(false);
    }
  };

  const handleSaveObservability = async () => {
    setObsBusy(true);
    try {
      const resp = await api.updateObservability(developerMode, logLevel);
      if (resp.ok) await loadObservability();
    } catch {
      // ignore
    } finally {
      setObsBusy(false);
    }
  };

  const loadDebugSnapshot = async () => {
    if (!groupId) return;
    setDebugSnapshotBusy(true);
    setDebugSnapshotErr("");
    try {
      const resp = await api.fetchDebugSnapshot(groupId);
      if (resp.ok) {
        setDebugSnapshot(JSON.stringify(resp.result ?? {}, null, 2));
      } else {
        setDebugSnapshot("");
        setDebugSnapshotErr(resp.error?.message || "Failed to load debug snapshot");
      }
    } catch {
      setDebugSnapshot("");
      setDebugSnapshotErr("Failed to load debug snapshot");
    } finally {
      setDebugSnapshotBusy(false);
    }
  };

  const loadLogTail = async () => {
    setLogBusy(true);
    setLogErr("");
    try {
      const resp = await api.fetchLogTail(logComponent, groupId || "", logLines || 200);
      if (resp.ok) {
        const lines = Array.isArray(resp.result?.lines) ? resp.result.lines : [];
        setLogText(lines.join("\n"));
      } else {
        setLogText("");
        setLogErr(resp.error?.message || "Failed to tail logs");
      }
    } catch {
      setLogText("");
      setLogErr("Failed to tail logs");
    } finally {
      setLogBusy(false);
    }
  };

  const handleClearLogs = async () => {
    if (!developerMode) return;
    if (logComponent === "im" && !groupId) {
      setLogErr("IM logs require a group_id; open Settings from a group.");
      return;
    }
    setLogBusy(true);
    setLogErr("");
    try {
      const resp = await api.clearLogs(logComponent, groupId || "");
      if (!resp.ok) {
        setLogErr(resp.error?.message || "Failed to clear logs");
        return;
      }
      setLogText("");
    } catch {
      setLogErr("Failed to clear logs");
    } finally {
      setLogBusy(false);
    }
  };

  // ============ Render ============

  if (!isOpen) return null;

  const groupTabs: { id: GroupTabId; label: string }[] = [
    { id: "timing", label: "Timing" },
    { id: "im", label: "IM Bridge" },
    { id: "transcript", label: "Transcript" },
  ];
  const globalTabs: { id: GlobalTabId; label: string }[] = [
    { id: "remote", label: "Remote Access" },
    { id: "developer", label: "Developer" },
  ];
  const tabs = scope === "group" ? groupTabs : globalTabs;
  const activeTab = scope === "group" ? groupTab : globalTab;
  const setActiveTab = (tab: GroupTabId | GlobalTabId) => {
    if (scope === "group") setGroupTab(tab as GroupTabId);
    else setGlobalTab(tab as GlobalTabId);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 animate-fade-in">
      {/* Backdrop */}
      <div
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"}
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Modal */}
      <div
        className={`relative rounded-xl border shadow-2xl w-full max-w-lg max-h-[80vh] flex flex-col animate-scale-in ${
          isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
        }`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
      >
        {/* Header */}
        <div className={`flex items-center justify-between px-5 py-4 border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          <h2 id="settings-modal-title" className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
            ⚙️ Settings
          </h2>
          <button
            onClick={onClose}
            className={`text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors ${
              isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
            }`}
            aria-label="Close settings"
          >
            ×
          </button>
        </div>

        {/* Scope Toggle */}
        <div className={`px-5 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setScope("group")}
              disabled={!groupId}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                scope === "group"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                    : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
              } disabled:opacity-40`}
            >
              This group
            </button>
            <button
              type="button"
              onClick={() => setScope("global")}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                scope === "global"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                    : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
              }`}
            >
              Global
            </button>
          </div>
          {scope === "group" && groupId && (
            <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              Applies to <span className="font-mono">{groupId}</span> only.
            </div>
          )}
          {scope === "global" && (
            <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              Applies to your whole CCCC instance (daemon + Web).
            </div>
          )}
        </div>

        {/* Tabs */}
        <div className={`flex border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2.5 text-sm font-medium transition-colors ${
                activeTab === tab.id
                  ? isDark
                    ? "text-emerald-400 border-b-2 border-emerald-400"
                    : "text-emerald-600 border-b-2 border-emerald-600"
                  : isDark
                    ? "text-slate-400 hover:text-slate-200"
                    : "text-gray-500 hover:text-gray-700"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {activeTab === "timing" && (
            <TimingTab
              isDark={isDark}
              busy={busy}
              nudgeSeconds={nudgeSeconds}
              setNudgeSeconds={setNudgeSeconds}
              idleSeconds={idleSeconds}
              setIdleSeconds={setIdleSeconds}
              keepaliveSeconds={keepaliveSeconds}
              setKeepaliveSeconds={setKeepaliveSeconds}
              silenceSeconds={silenceSeconds}
              setSilenceSeconds={setSilenceSeconds}
              deliveryInterval={deliveryInterval}
              setDeliveryInterval={setDeliveryInterval}
              standupInterval={standupInterval}
              setStandupInterval={setStandupInterval}
              onSave={handleSaveSettings}
            />
          )}

          {activeTab === "im" && (
            <IMBridgeTab
              isDark={isDark}
              groupId={groupId}
              imStatus={imStatus}
              imPlatform={imPlatform}
              setImPlatform={setImPlatform}
              imBotTokenEnv={imBotTokenEnv}
              setImBotTokenEnv={setImBotTokenEnv}
              imAppTokenEnv={imAppTokenEnv}
              setImAppTokenEnv={setImAppTokenEnv}
              imBusy={imBusy}
              onSaveConfig={handleSaveIMConfig}
              onRemoveConfig={handleRemoveIMConfig}
              onStartBridge={handleStartBridge}
              onStopBridge={handleStopBridge}
            />
          )}

          {activeTab === "transcript" && (
            <TranscriptTab
              isDark={isDark}
              busy={busy}
              groupId={groupId}
              devActors={devActors}
              terminalVisibility={terminalVisibility}
              setTerminalVisibility={setTerminalVisibility}
              terminalNotifyTail={terminalNotifyTail}
              setTerminalNotifyTail={setTerminalNotifyTail}
              terminalNotifyLines={terminalNotifyLines}
              setTerminalNotifyLines={setTerminalNotifyLines}
              onSaveTranscriptSettings={handleSaveTranscriptSettings}
              tailActorId={tailActorId}
              setTailActorId={setTailActorId}
              tailMaxChars={tailMaxChars}
              setTailMaxChars={setTailMaxChars}
              tailStripAnsi={tailStripAnsi}
              setTailStripAnsi={setTailStripAnsi}
              tailCompact={tailCompact}
              setTailCompact={setTailCompact}
              tailText={tailText}
              tailHint={tailHint}
              tailErr={tailErr}
              tailBusy={tailBusy}
              tailCopyInfo={tailCopyInfo}
              onLoadTail={loadTerminalTail}
              onCopyTail={copyTailLastLines}
              onClearTail={clearTail}
            />
          )}

          {activeTab === "remote" && <RemoteAccessTab isDark={isDark} />}

          {activeTab === "developer" && (
            <DeveloperTab
              isDark={isDark}
              groupId={groupId}
              developerMode={developerMode}
              setDeveloperMode={setDeveloperMode}
              logLevel={logLevel}
              setLogLevel={setLogLevel}
              obsBusy={obsBusy}
              onSaveObservability={handleSaveObservability}
              debugSnapshot={debugSnapshot}
              debugSnapshotErr={debugSnapshotErr}
              debugSnapshotBusy={debugSnapshotBusy}
              onLoadDebugSnapshot={loadDebugSnapshot}
              onClearDebugSnapshot={() => {
                setDebugSnapshot("");
                setDebugSnapshotErr("");
              }}
              logComponent={logComponent}
              setLogComponent={setLogComponent}
              logLines={logLines}
              setLogLines={setLogLines}
              logText={logText}
              logErr={logErr}
              logBusy={logBusy}
              onLoadLogTail={loadLogTail}
              onClearLogs={handleClearLogs}
            />
          )}
        </div>
      </div>
    </div>
  );
}
