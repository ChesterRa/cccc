// SettingsModal renders the settings modal.
import { useState, useEffect, useRef, useCallback } from "react";
import { Actor, GroupDoc, GroupSettings, IMStatus, IMPlatform } from "../types";
import * as api from "../services/api";
import { useObservabilityStore } from "../stores";
import {
  AutomationTab,
  DeliveryTab,
  MessagingTab,
  IMBridgeTab,
  TranscriptTab,
  GuidanceTab,
  BlueprintTab,
  RemoteAccessTab,
  DeveloperTab,
  SettingsScope,
  GroupTabId,
  GlobalTabId,
} from "./modals/settings";
import { InfoIcon } from "./Icons";
import { ScrollFade } from "./ScrollFade";
import { useModalA11y } from "../hooks/useModalA11y";
import {
  useFloating,
  useHover,
  useDismiss,
  useRole,
  useInteractions,
  FloatingPortal,
  offset,
  flip,
  shift,
  autoUpdate,
} from "@floating-ui/react";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  settings: GroupSettings | null;
  onUpdateSettings: (settings: Partial<GroupSettings>) => Promise<void>;
  onRegistryChanged?: () => Promise<void> | void;
  busy: boolean;
  isDark: boolean;
  groupId?: string;
  groupDoc?: GroupDoc | null;
}

function ScopeTooltip({
  isDark,
  title,
  content,
  children,
}: {
  isDark: boolean;
  title: string;
  content: React.ReactNode;
  children: (getReferenceProps: (userProps?: React.ButtonHTMLAttributes<HTMLButtonElement>) => Record<string, unknown>, setReference: (node: HTMLElement | null) => void) => React.ReactNode;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const { refs, floatingStyles, context } = useFloating({
    open: isOpen,
    onOpenChange: setIsOpen,
    placement: "top",
    middleware: [offset(8), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
    strategy: "fixed",
  });

  const isPositioned = context.isPositioned;

  const hover = useHover(context, { delay: 150, restMs: 100 });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "tooltip" });

  const { getReferenceProps, getFloatingProps } = useInteractions([hover, dismiss, role]);

  const setReference = useCallback((node: HTMLElement | null) => {
    refs.setReference(node);
  }, [refs]);

  const setFloating = useCallback((node: HTMLElement | null) => {
    refs.setFloating(node);
  }, [refs]);

  return (
    <>
      {children(getReferenceProps, setReference)}
      <FloatingPortal>
        {isOpen && (
          <div
            ref={setFloating}
            style={floatingStyles}
            {...getFloatingProps()}
            className={`z-max w-max max-w-[220px] rounded-lg border shadow-xl px-3 py-2 text-[11px] transition-opacity duration-150 ${isPositioned ? "opacity-100" : "opacity-0"
              } ${isDark ? "bg-slate-900 border-slate-700 text-slate-300" : "bg-white border-gray-200 text-gray-600"
              }`}
          >
            <div className="font-semibold mb-1 text-emerald-500">{title}</div>
            {content}
          </div>
        )}
      </FloatingPortal>
    </>
  );
}

export function SettingsModal({
  isOpen,
  onClose,
  settings,
  onUpdateSettings,
  onRegistryChanged,
  busy,
  isDark,
  groupId,
  groupDoc,
}: SettingsModalProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [scope, setScope] = useState<SettingsScope>(groupId ? "group" : "global");
  const [groupTab, setGroupTab] = useState<GroupTabId>("automation");
  const [globalTab, setGlobalTab] = useState<GlobalTabId>("remote");

  // Automation + delivery settings state
  const [nudgeSeconds, setNudgeSeconds] = useState(300);
  const [replyRequiredNudgeSeconds, setReplyRequiredNudgeSeconds] = useState(300);
  const [attentionAckNudgeSeconds, setAttentionAckNudgeSeconds] = useState(600);
  const [unreadNudgeSeconds, setUnreadNudgeSeconds] = useState(900);
  const [nudgeDigestMinIntervalSeconds, setNudgeDigestMinIntervalSeconds] = useState(120);
  const [nudgeMaxRepeatsPerObligation, setNudgeMaxRepeatsPerObligation] = useState(3);
  const [nudgeEscalateAfterRepeats, setNudgeEscalateAfterRepeats] = useState(2);
  const [idleSeconds, setIdleSeconds] = useState(600);
  const [keepaliveSeconds, setKeepaliveSeconds] = useState(120);
  const [keepaliveMax, setKeepaliveMax] = useState(3);
  const [silenceSeconds, setSilenceSeconds] = useState(600);
  const [helpNudgeIntervalSeconds, setHelpNudgeIntervalSeconds] = useState(600);
  const [helpNudgeMinMessages, setHelpNudgeMinMessages] = useState(10);
  const [deliveryInterval, setDeliveryInterval] = useState(0);
  const [autoMarkOnDelivery, setAutoMarkOnDelivery] = useState(false);

  // Messaging policy
  const [defaultSendTo, setDefaultSendTo] = useState<"foreman" | "broadcast">("foreman");

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
  const [imPlatform, setImPlatform] = useState<IMPlatform>("telegram");
  const [imBotTokenEnv, setImBotTokenEnv] = useState("");
  const [imAppTokenEnv, setImAppTokenEnv] = useState("");
  // Feishu fields
  const [imFeishuDomain, setImFeishuDomain] = useState("https://open.feishu.cn");
  const [imFeishuAppId, setImFeishuAppId] = useState("");
  const [imFeishuAppSecret, setImFeishuAppSecret] = useState("");
  // DingTalk fields
  const [imDingtalkAppKey, setImDingtalkAppKey] = useState("");
  const [imDingtalkAppSecret, setImDingtalkAppSecret] = useState("");
  const [imDingtalkRobotCode, setImDingtalkRobotCode] = useState("");
  const [imBusy, setImBusy] = useState(false);
  const imLoadSeq = useRef(0);

  // IM config drafts cache (per-platform local edits, not yet saved to server)
  type IMConfigDraft = {
    botTokenEnv: string;
    appTokenEnv: string;
    feishuDomain: string;
    feishuAppId: string;
    feishuAppSecret: string;
    dingtalkAppKey: string;
    dingtalkAppSecret: string;
    dingtalkRobotCode: string;
  };
  const [imConfigDrafts, setImConfigDrafts] = useState<Partial<Record<IMPlatform, IMConfigDraft>>>({});

  // Global observability (developer mode)
  const [developerMode, setDeveloperMode] = useState(false);
  const [logLevel, setLogLevel] = useState<"INFO" | "DEBUG">("INFO");
  const [terminalBacklogMiB, setTerminalBacklogMiB] = useState(10);
  const [terminalScrollbackLines, setTerminalScrollbackLines] = useState(8000);
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

  // Registry maintenance (global)
  const [registryBusy, setRegistryBusy] = useState(false);
  const [registryErr, setRegistryErr] = useState("");
  const [registryResult, setRegistryResult] = useState<api.RegistryReconcileResult | null>(null);

  // ============ Effects ============

  useEffect(() => {
    if (isOpen && settings) {
      setNudgeSeconds(settings.nudge_after_seconds);
      setReplyRequiredNudgeSeconds(settings.reply_required_nudge_after_seconds ?? 300);
      setAttentionAckNudgeSeconds(settings.attention_ack_nudge_after_seconds ?? 600);
      setUnreadNudgeSeconds(settings.unread_nudge_after_seconds ?? 900);
      setNudgeDigestMinIntervalSeconds(settings.nudge_digest_min_interval_seconds ?? 120);
      setNudgeMaxRepeatsPerObligation(settings.nudge_max_repeats_per_obligation ?? 3);
      setNudgeEscalateAfterRepeats(settings.nudge_escalate_after_repeats ?? 2);
      setIdleSeconds(settings.actor_idle_timeout_seconds);
      setKeepaliveSeconds(settings.keepalive_delay_seconds);
      setKeepaliveMax(settings.keepalive_max_per_actor ?? 3);
      setSilenceSeconds(settings.silence_timeout_seconds);
      setHelpNudgeIntervalSeconds(settings.help_nudge_interval_seconds ?? 600);
      setHelpNudgeMinMessages(settings.help_nudge_min_messages ?? 10);
      setDeliveryInterval(settings.min_interval_seconds);
      setAutoMarkOnDelivery(Boolean(settings.auto_mark_on_delivery));
      setDefaultSendTo(settings.default_send_to || "foreman");
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
    if (!isOpen) return;
    loadIMStatus({ resetFirst: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Only load when the modal opens or groupId changes.
  }, [isOpen, groupId]);

  useEffect(() => {
    if (isOpen) loadObservability();
  }, [isOpen]);

  useEffect(() => {
    if (isOpen && groupId) loadDevActors();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Only load when the modal opens or groupId changes.
  }, [isOpen, groupId]);

  useEffect(() => {
    if (!isOpen) return;
    if (scope !== "global" || globalTab !== "developer") return;
    void loadRegistryPreview();
  }, [isOpen, scope, globalTab]);

  // ============ Data Loading ============

  const resetIMState = () => {
    setImStatus(null);
    setImPlatform("telegram");
    setImBotTokenEnv("");
    setImAppTokenEnv("");
    setImFeishuDomain("https://open.feishu.cn");
    setImFeishuAppId("");
    setImFeishuAppSecret("");
    setImDingtalkAppKey("");
    setImDingtalkAppSecret("");
    setImDingtalkRobotCode("");
  };

  const loadIMStatus = async (opts?: { resetFirst?: boolean }) => {
    const gid = String(groupId || "").trim();
    const seq = ++imLoadSeq.current;
    if (opts?.resetFirst) resetIMState();
    if (!gid) return;
    try {
      const statusResp = await api.fetchIMStatus(gid);
      if (seq !== imLoadSeq.current) return;
      if (statusResp.ok) {
        setImStatus(statusResp.result);
        if (statusResp.result.platform) {
          setImPlatform(statusResp.result.platform as IMPlatform);
        }
      }
      const configResp = await api.fetchIMConfig(gid);
      if (seq !== imLoadSeq.current) return;
      if (configResp.ok && configResp.result.im) {
        const im = configResp.result.im;
        if (im.platform) setImPlatform(im.platform);
        setImBotTokenEnv(im.bot_token_env || im.token_env || im.token || "");
        setImAppTokenEnv(im.app_token_env || "");
        // Feishu fields
        {
          const raw = String(im.feishu_domain || "https://open.feishu.cn").trim();
          const canon = raw
            .replace(/\/+$/, "")
            .replace(/\/open-apis$/, "")
            .replace(/^open\.larksuite\.com$/i, "https://open.larkoffice.com")
            .replace(/^https?:\/\/open\.larksuite\.com$/i, "https://open.larkoffice.com")
            .replace(/^open\.larkoffice\.com$/i, "https://open.larkoffice.com");
          setImFeishuDomain(canon);
        }
        setImFeishuAppId(im.feishu_app_id || im.feishu_app_id_env || "");
        setImFeishuAppSecret(im.feishu_app_secret || im.feishu_app_secret_env || "");
        // DingTalk fields
        setImDingtalkAppKey(im.dingtalk_app_key || im.dingtalk_app_key_env || "");
        setImDingtalkAppSecret(im.dingtalk_app_secret || im.dingtalk_app_secret_env || "");
        setImDingtalkRobotCode(im.dingtalk_robot_code || im.dingtalk_robot_code_env || "");
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

  const handleSaveDeliverySettings = async () => {
    await onUpdateSettings({
      min_interval_seconds: deliveryInterval,
      auto_mark_on_delivery: autoMarkOnDelivery,
    });
  };

  const handleSaveAutomationSettings = async () => {
    await onUpdateSettings({
      nudge_after_seconds: nudgeSeconds,
      reply_required_nudge_after_seconds: replyRequiredNudgeSeconds,
      attention_ack_nudge_after_seconds: attentionAckNudgeSeconds,
      unread_nudge_after_seconds: unreadNudgeSeconds,
      nudge_digest_min_interval_seconds: nudgeDigestMinIntervalSeconds,
      nudge_max_repeats_per_obligation: nudgeMaxRepeatsPerObligation,
      nudge_escalate_after_repeats: nudgeEscalateAfterRepeats,
      actor_idle_timeout_seconds: idleSeconds,
      keepalive_delay_seconds: keepaliveSeconds,
      keepalive_max_per_actor: keepaliveMax,
      silence_timeout_seconds: silenceSeconds,
      help_nudge_interval_seconds: helpNudgeIntervalSeconds,
      help_nudge_min_messages: helpNudgeMinMessages,
    });
  };

  const handleAutoSave = async (field: string, value: number | boolean) => {
    await onUpdateSettings({ [field]: value });
  };

  const handleSaveTranscriptSettings = async () => {
    await onUpdateSettings({
      terminal_transcript_visibility: terminalVisibility,
      terminal_transcript_notify_tail: terminalNotifyTail,
      terminal_transcript_notify_lines: terminalNotifyLines,
    });
  };

  const handleSaveMessagingSettings = async () => {
    await onUpdateSettings({
      default_send_to: defaultSendTo,
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

  // Get current IM config as a draft object
  const getCurrentIMConfigDraft = (): IMConfigDraft => ({
    botTokenEnv: imBotTokenEnv,
    appTokenEnv: imAppTokenEnv,
    feishuDomain: imFeishuDomain,
    feishuAppId: imFeishuAppId,
    feishuAppSecret: imFeishuAppSecret,
    dingtalkAppKey: imDingtalkAppKey,
    dingtalkAppSecret: imDingtalkAppSecret,
    dingtalkRobotCode: imDingtalkRobotCode,
  });

  // Apply a draft to current IM config fields
  const applyIMConfigDraft = (draft: IMConfigDraft) => {
    setImBotTokenEnv(draft.botTokenEnv);
    setImAppTokenEnv(draft.appTokenEnv);
    setImFeishuDomain(draft.feishuDomain);
    setImFeishuAppId(draft.feishuAppId);
    setImFeishuAppSecret(draft.feishuAppSecret);
    setImDingtalkAppKey(draft.dingtalkAppKey);
    setImDingtalkAppSecret(draft.dingtalkAppSecret);
    setImDingtalkRobotCode(draft.dingtalkRobotCode);
  };

  // Handle platform change with config caching
  const handlePlatformChange = (newPlatform: IMPlatform) => {
    if (newPlatform === imPlatform) return;

    // 1. Save current platform config to drafts
    setImConfigDrafts((prev) => ({
      ...prev,
      [imPlatform]: getCurrentIMConfigDraft(),
    }));

    // 2. Load new platform's cached draft (if exists)
    const cachedDraft = imConfigDrafts[newPlatform];
    if (cachedDraft) {
      applyIMConfigDraft(cachedDraft);
    } else {
      // Reset to empty if no cached draft (new platform)
      setImBotTokenEnv("");
      setImAppTokenEnv("");
      setImFeishuDomain("https://open.feishu.cn");
      setImFeishuAppId("");
      setImFeishuAppSecret("");
      setImDingtalkAppKey("");
      setImDingtalkAppSecret("");
      setImDingtalkRobotCode("");
    }

    // 3. Set new platform
    setImPlatform(newPlatform);
  };

  const handleSaveIMConfig = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      const resp = await api.setIMConfig(groupId, imPlatform, imBotTokenEnv, imAppTokenEnv, {
        feishu_domain: imFeishuDomain,
        feishu_app_id: imFeishuAppId,
        feishu_app_secret: imFeishuAppSecret,
        dingtalk_app_key: imDingtalkAppKey,
        dingtalk_app_secret: imDingtalkAppSecret,
        dingtalk_robot_code: imDingtalkRobotCode,
      });
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
        setImFeishuDomain("https://open.feishu.cn");
        setImFeishuAppId("");
        setImFeishuAppSecret("");
        setImDingtalkAppKey("");
        setImDingtalkAppSecret("");
        setImDingtalkRobotCode("");
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
      const perActorBytes = Math.max(1, Math.min(50, Number(terminalBacklogMiB || 0))) * 1024 * 1024;
      const scrollbackLines = Math.max(1000, Math.min(200000, Number(terminalScrollbackLines || 0)));
      const resp = await api.updateObservability({
        developerMode,
        logLevel,
        terminalTranscriptPerActorBytes: perActorBytes,
        terminalUiScrollbackLines: scrollbackLines,
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

  const handleReconcileRegistry = async () => {
    const missingCount = registryResult?.missing_group_ids?.length || 0;
    if (missingCount <= 0) {
      await loadRegistryPreview();
      return;
    }
    if (!window.confirm(`Remove ${missingCount} missing group entries from registry?`)) {
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

  // ============ Render ============

  if (!isOpen) return null;

  const scopeRootUrl = (() => {
    if (!groupDoc || String(groupDoc.group_id || "") !== String(groupId || "")) return "";
    const scopes = Array.isArray(groupDoc.scopes) ? groupDoc.scopes : [];
    const activeKey = String(groupDoc.active_scope_key || "");
    const active = scopes.find((s) => String(s?.scope_key || "") === activeKey && String(s?.url || "").trim());
    const first = scopes.find((s) => String(s?.url || "").trim());
    return String((active || first)?.url || "").trim();
  })();

  const groupTabs: { id: GroupTabId; label: string }[] = [
    { id: "guidance", label: "Guidance" },
    { id: "automation", label: "Automation" },
    { id: "delivery", label: "Delivery" },
    { id: "messaging", label: "Messaging" },
    { id: "im", label: "IM Bridge" },
    { id: "transcript", label: "Transcript" },
    { id: "blueprint", label: "Blueprint" },
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
    <div className="fixed inset-0 z-50 flex items-stretch sm:items-center justify-center p-0 sm:p-4 animate-fade-in">
      {/* Backdrop */}
      <div
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"}
        onPointerDown={onClose}
        aria-hidden="true"
      />

      {/* Modal */}
      <div
        className={`relative w-full h-full sm:h-[640px] sm:max-w-4xl sm:max-h-[85vh] flex flex-col border shadow-2xl animate-scale-in rounded-none sm:rounded-xl ${isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
          }`}
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
      >
        {/* Header */}
        <div className={`flex flex-shrink-0 items-center justify-between px-5 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          <h2 id="settings-modal-title" className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
            ⚙️ Settings
          </h2>
          <button
            onClick={onClose}
            className={`text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors ${isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
              }`}
            aria-label="Close settings"
          >
            ×
          </button>
        </div>

        <div className="flex-1 flex flex-col sm:flex-row overflow-hidden">
          {/* Desktop Sidebar Navigation */}
          <aside className={`hidden sm:flex sm:flex-col w-48 border-r flex-shrink-0 ${isDark ? "bg-slate-900/50 border-slate-800" : "bg-gray-50/50 border-gray-100"}`}>
            {/* Desktop Scope Toggle */}
            <div className="p-3 space-y-3">
              <div className={`px-3 text-[10px] font-bold uppercase tracking-wider opacity-30 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
                Target Scope
              </div>
              <div className="flex flex-col gap-1">
                <button
                  type="button"
                  onClick={() => setScope("group")}
                  disabled={!groupId}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-sm text-left font-semibold transition-colors ${scope === "group"
                    ? isDark
                      ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                      : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                    : isDark
                      ? "hover:bg-slate-800 text-slate-400"
                      : "hover:bg-gray-100 text-gray-600"
                    } disabled:opacity-40`}
                >
                  <span>This group</span>
                  <ScopeTooltip
                    isDark={isDark}
                    title="Group Scope"
                    content={<>Applies to <span className="font-mono text-emerald-500">{scopeRootUrl || groupId}</span> only. Useful for group-specific timeouts and integrations.</>}
                  >
                    {(getReferenceProps, setReference) => (
                      <div
                        ref={setReference}
                        {...getReferenceProps({
                          onClick: (e) => e.stopPropagation()
                        })}
                        className="p-1 -mr-1 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                      >
                        <InfoIcon size={12} />
                      </div>
                    )}
                  </ScopeTooltip>
                </button>

                <button
                  type="button"
                  onClick={() => setScope("global")}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-sm text-left font-semibold transition-colors ${scope === "global"
                    ? isDark
                      ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                      : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                    : isDark
                      ? "hover:bg-slate-800 text-slate-400"
                      : "hover:bg-gray-100 text-gray-600"
                    }`}
                >
                  <span>Global</span>
                  <ScopeTooltip
                    isDark={isDark}
                    title="Global Scope"
                    content={<>Applies to your whole CCCC instance (daemon + Web). These settings affect all groups unless overridden.</>}
                  >
                    {(getReferenceProps, setReference) => (
                      <div
                        ref={setReference}
                        {...getReferenceProps({
                          onClick: (e) => e.stopPropagation()
                        })}
                        className="p-1 -mr-1 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                      >
                        <InfoIcon size={12} />
                      </div>
                    )}
                  </ScopeTooltip>
                </button>
              </div>
            </div>

            <div className={`mx-3 border-b ${isDark ? "border-slate-800" : "border-gray-100"}`} />

            {/* Desktop Vertical Tabs */}
            <nav className="flex-1 overflow-y-auto p-3 space-y-1">
              {tabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`w-full flex items-center px-3 py-2 text-xs font-medium rounded-lg transition-colors ${activeTab === tab.id
                    ? isDark
                      ? "bg-slate-800 text-emerald-400"
                      : "bg-white shadow-sm border border-gray-200 text-emerald-600"
                    : isDark
                      ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800/50"
                      : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                    }`}
                >
                  {tab.label}
                </button>
              ))}
            </nav>
          </aside>

          {/* Mobile Navigation (Header-style) */}
          <div className="sm:hidden flex flex-col flex-shrink-0">
            {/* Mobile Scope Toggle */}
            <div className={`px-5 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setScope("group")}
                  disabled={!groupId}
                  className={`flex-1 relative flex items-center justify-center px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${scope === "group"
                    ? isDark
                      ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                      : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                    : isDark
                      ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                      : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
                    } disabled:opacity-40`}
                >
                  <span>This group</span>
                  <div className="absolute right-1 top-1/2 -translate-y-1/2">
                    <ScopeTooltip
                      isDark={isDark}
                      title="Group Scope"
                      content={<>Applies to <span className="font-mono text-emerald-500">{scopeRootUrl || groupId}</span> only. Useful for group-specific timeouts and integrations.</>}
                    >
                      {(getReferenceProps, setReference) => (
                        <div
                          ref={setReference}
                          {...getReferenceProps({
                            onClick: (e) => e.stopPropagation()
                          })}
                          className="p-1.5 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                        >
                          <InfoIcon size={14} />
                        </div>
                      )}
                    </ScopeTooltip>
                  </div>
                </button>

                <button
                  type="button"
                  onClick={() => setScope("global")}
                  className={`flex-1 relative flex items-center justify-center px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${scope === "global"
                    ? isDark
                      ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                      : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                    : isDark
                      ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                      : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
                    }`}
                >
                  <span>Global</span>
                  <div className="absolute right-1 top-1/2 -translate-y-1/2">
                    <ScopeTooltip
                      isDark={isDark}
                      title="Global Scope"
                      content={<>Applies to your whole CCCC instance (daemon + Web). These settings affect all groups unless overridden.</>}
                    >
                      {(getReferenceProps, setReference) => (
                        <div
                          ref={setReference}
                          {...getReferenceProps({
                            onClick: (e) => e.stopPropagation()
                          })}
                          className="p-1.5 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                        >
                          <InfoIcon size={14} />
                        </div>
                      )}
                    </ScopeTooltip>
                  </div>
                </button>
              </div>
            </div>

            {/* Mobile Tabs - Horizontally scrollable with fade edges */}
            <ScrollFade
              className={`flex-shrink-0 w-full border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}
              innerClassName="flex min-h-[48px]"
              fadeWidth={20}
            >
              {tabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`flex-shrink-0 px-4 py-2.5 text-xs font-medium transition-colors whitespace-nowrap ${activeTab === tab.id
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
            </ScrollFade>
          </div>

          {/* Main Content Area */}
          <div className="flex-1 overflow-y-auto flex flex-col">
            <div className="p-5 sm:p-8 space-y-6">
              {activeTab === "automation" && (
                <AutomationTab
                  isDark={isDark}
                  groupId={groupId}
                  devActors={devActors}
                  busy={busy}
                  nudgeSeconds={nudgeSeconds}
                  setNudgeSeconds={setNudgeSeconds}
                  replyRequiredNudgeSeconds={replyRequiredNudgeSeconds}
                  setReplyRequiredNudgeSeconds={setReplyRequiredNudgeSeconds}
                  attentionAckNudgeSeconds={attentionAckNudgeSeconds}
                  setAttentionAckNudgeSeconds={setAttentionAckNudgeSeconds}
                  unreadNudgeSeconds={unreadNudgeSeconds}
                  setUnreadNudgeSeconds={setUnreadNudgeSeconds}
                  nudgeDigestMinIntervalSeconds={nudgeDigestMinIntervalSeconds}
                  setNudgeDigestMinIntervalSeconds={setNudgeDigestMinIntervalSeconds}
                  nudgeMaxRepeatsPerObligation={nudgeMaxRepeatsPerObligation}
                  setNudgeMaxRepeatsPerObligation={setNudgeMaxRepeatsPerObligation}
                  nudgeEscalateAfterRepeats={nudgeEscalateAfterRepeats}
                  setNudgeEscalateAfterRepeats={setNudgeEscalateAfterRepeats}
                  idleSeconds={idleSeconds}
                  setIdleSeconds={setIdleSeconds}
                  keepaliveSeconds={keepaliveSeconds}
                  setKeepaliveSeconds={setKeepaliveSeconds}
                  keepaliveMax={keepaliveMax}
                  setKeepaliveMax={setKeepaliveMax}
                  silenceSeconds={silenceSeconds}
                  setSilenceSeconds={setSilenceSeconds}
                  helpNudgeIntervalSeconds={helpNudgeIntervalSeconds}
                  setHelpNudgeIntervalSeconds={setHelpNudgeIntervalSeconds}
                  helpNudgeMinMessages={helpNudgeMinMessages}
                  setHelpNudgeMinMessages={setHelpNudgeMinMessages}
                  onSavePolicies={handleSaveAutomationSettings}
                />
              )}

              {activeTab === "delivery" && (
                <DeliveryTab
                  isDark={isDark}
                  busy={busy}
                  deliveryInterval={deliveryInterval}
                  setDeliveryInterval={setDeliveryInterval}
                  autoMarkOnDelivery={autoMarkOnDelivery}
                  setAutoMarkOnDelivery={setAutoMarkOnDelivery}
                  onSave={handleSaveDeliverySettings}
                  onAutoSave={handleAutoSave}
                />
              )}

              {activeTab === "messaging" && (
                <MessagingTab
                  isDark={isDark}
                  busy={busy}
                  defaultSendTo={defaultSendTo}
                  setDefaultSendTo={setDefaultSendTo}
                  onSave={handleSaveMessagingSettings}
                />
              )}

              {activeTab === "im" && (
                <IMBridgeTab
                  isDark={isDark}
                  groupId={groupId}
                  imStatus={imStatus}
                  imPlatform={imPlatform}
                  onPlatformChange={handlePlatformChange}
                  imBotTokenEnv={imBotTokenEnv}
                  setImBotTokenEnv={setImBotTokenEnv}
                  imAppTokenEnv={imAppTokenEnv}
                  setImAppTokenEnv={setImAppTokenEnv}
                  imFeishuAppId={imFeishuAppId}
                  setImFeishuAppId={setImFeishuAppId}
                  imFeishuAppSecret={imFeishuAppSecret}
                  setImFeishuAppSecret={setImFeishuAppSecret}
                  imFeishuDomain={imFeishuDomain}
                  setImFeishuDomain={setImFeishuDomain}
                  imDingtalkAppKey={imDingtalkAppKey}
                  setImDingtalkAppKey={setImDingtalkAppKey}
                  imDingtalkAppSecret={imDingtalkAppSecret}
                  setImDingtalkAppSecret={setImDingtalkAppSecret}
                  imDingtalkRobotCode={imDingtalkRobotCode}
                  setImDingtalkRobotCode={setImDingtalkRobotCode}
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

              {activeTab === "guidance" && <GuidanceTab isDark={isDark} groupId={groupId} />}

              {activeTab === "blueprint" && <BlueprintTab isDark={isDark} groupId={groupId} groupTitle={groupDoc?.title || ""} />}

              {activeTab === "remote" && <RemoteAccessTab isDark={isDark} />}

              {activeTab === "developer" && (
                <DeveloperTab
                  isDark={isDark}
                  groupId={groupId}
                  developerMode={developerMode}
                  setDeveloperMode={setDeveloperMode}
                  logLevel={logLevel}
                  setLogLevel={setLogLevel}
                  terminalBacklogMiB={terminalBacklogMiB}
                  setTerminalBacklogMiB={setTerminalBacklogMiB}
                  terminalScrollbackLines={terminalScrollbackLines}
                  setTerminalScrollbackLines={setTerminalScrollbackLines}
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
                  registryBusy={registryBusy}
                  registryErr={registryErr}
                  registryResult={registryResult}
                  onPreviewRegistry={loadRegistryPreview}
                  onReconcileRegistry={handleReconcileRegistry}
                />
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
