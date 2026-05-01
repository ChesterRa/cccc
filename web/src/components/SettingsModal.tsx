// SettingsModal renders the settings modal.
import { lazy, Suspense, useState, useEffect, useRef, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Actor, GroupDoc, GroupSettings, IMStatus, IMPlatform, WebAccessSession, WeixinLoginStatus } from "../types";
import * as api from "../services/api";
import { useObservabilityStore } from "../stores";
import type { RuntimeVisibilityMode } from "../utils/runtimeVisibility";
import {
  SettingsScope,
  GroupTabId,
  GlobalTabId,
} from "./modals/settings/types";
import { ModalFrame } from "./modals/ModalFrame";
import { SettingsNavigation } from "./modals/settings/SettingsNavigation";
import { IMConfigDraft, saveAndStartIMBridge, saveIMConfigDraft } from "./modals/settings/imBridgeConfig";
import { useModalA11y } from "../hooks/useModalA11y";
import { copyTextToClipboard } from "../utils/copy";

const AutomationTab = lazy(() => import("./modals/settings/AutomationTab").then((module) => ({ default: module.AutomationTab })));
const DeliveryTab = lazy(() => import("./modals/settings/DeliveryTab").then((module) => ({ default: module.DeliveryTab })));
const MessagingTab = lazy(() => import("./modals/settings/MessagingTab").then((module) => ({ default: module.MessagingTab })));
const IMBridgeTab = lazy(() => import("./modals/settings/IMBridgeTab").then((module) => ({ default: module.IMBridgeTab })));
const TranscriptTab = lazy(() => import("./modals/settings/TranscriptTab").then((module) => ({ default: module.TranscriptTab })));
const GuidanceTab = lazy(() => import("./modals/settings/GuidanceTab").then((module) => ({ default: module.GuidanceTab })));
const AssistantsTab = lazy(() => import("./modals/settings/AssistantsTab").then((module) => ({ default: module.AssistantsTab })));
const GroupSpaceTab = lazy(() => import("./modals/settings/GroupSpaceTab").then((module) => ({ default: module.GroupSpaceTab })));
const BlueprintTab = lazy(() => import("./modals/settings/BlueprintTab").then((module) => ({ default: module.BlueprintTab })));
const CapabilitiesTab = lazy(() => import("./modals/settings/CapabilitiesTab").then((module) => ({ default: module.CapabilitiesTab })));
const ActorProfilesTab = lazy(() => import("./modals/settings/ActorProfilesTab").then((module) => ({ default: module.ActorProfilesTab })));
const BrandingTab = lazy(() => import("./modals/settings/BrandingTab").then((module) => ({ default: module.BrandingTab })));
const WebAccessTab = lazy(() => import("./modals/settings/WebAccessTab").then((module) => ({ default: module.WebAccessTab })));
const WebModelConnectorsTab = lazy(() =>
  import("./modals/settings/WebModelConnectorsTab").then((module) => ({ default: module.default }))
);
const DeveloperTab = lazy(() => import("./modals/settings/DeveloperTab").then((module) => ({ default: module.DeveloperTab })));

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  settings: GroupSettings | null;
  onUpdateSettings: (settings: Partial<GroupSettings>) => Promise<boolean | void>;
  onRegistryChanged?: () => Promise<void> | void;
  busy: boolean;
  isDark: boolean;
  groupId?: string;
  groupDoc?: GroupDoc | null;
}

function SettingsTabFallback({ isDark }: { isDark: boolean }) {
  return (
    <div
      className={`rounded-2xl border p-5 text-sm ${
        isDark ? "border-slate-700 bg-slate-900/70 text-slate-200" : "border-slate-200 bg-slate-50 text-slate-600"
      }`}
    >
      Loading...
    </div>
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
  const { t } = useTranslation("settings");
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [scope, setScope] = useState<SettingsScope>(groupId ? "group" : "global");
  const [groupTab, setGroupTab] = useState<GroupTabId>("guidance");
  const [globalTab, setGlobalTab] = useState<GlobalTabId>("capabilities");
  const [canAccessGlobalSettings, setCanAccessGlobalSettings] = useState<boolean | null>(null);
  const [webAccessSession, setWebAccessSession] = useState<WebAccessSession | null>(null);

  // Automation + delivery settings state
  const [nudgeSeconds, setNudgeSeconds] = useState(300);
  const [replyRequiredNudgeSeconds, setReplyRequiredNudgeSeconds] = useState(300);
  const [attentionAckNudgeSeconds, setAttentionAckNudgeSeconds] = useState(600);
  const [unreadNudgeSeconds, setUnreadNudgeSeconds] = useState(900);
  const [nudgeDigestMinIntervalSeconds, setNudgeDigestMinIntervalSeconds] = useState(120);
  const [nudgeMaxRepeatsPerObligation, setNudgeMaxRepeatsPerObligation] = useState(3);
  const [nudgeEscalateAfterRepeats, setNudgeEscalateAfterRepeats] = useState(2);
  const [idleSeconds, setIdleSeconds] = useState(0);
  const [keepaliveSeconds, setKeepaliveSeconds] = useState(120);
  const [keepaliveMax, setKeepaliveMax] = useState(3);
  const [silenceSeconds, setSilenceSeconds] = useState(0);
  const [helpNudgeIntervalSeconds, setHelpNudgeIntervalSeconds] = useState(600);
  const [helpNudgeMinMessages, setHelpNudgeMinMessages] = useState(10);
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
  // WeCom fields
  const [imWecomBotId, setImWecomBotId] = useState("");
  const [imWecomSecret, setImWecomSecret] = useState("");
  // Weixin fields
  const [imWeixinAccountId, setImWeixinAccountId] = useState("");
  const [weixinLoginStatus, setWeixinLoginStatus] = useState<WeixinLoginStatus | null>(null);
  const [imBusy, setImBusy] = useState(false);
  const imLoadSeq = useRef(0);
  const weixinAutoStartRef = useRef(false);
  const contentScrollRef = useRef<HTMLDivElement | null>(null);

  // IM config drafts cache (per-platform local edits, not yet saved to server)
  const [imConfigDrafts, setImConfigDrafts] = useState<Partial<Record<IMPlatform, IMConfigDraft>>>({});

  // Global observability (developer mode)
  const [developerMode, setDeveloperMode] = useState(false);
  const [logLevel, setLogLevel] = useState<"INFO" | "DEBUG">("INFO");
  const [terminalBacklogMiB, setTerminalBacklogMiB] = useState(10);
  const [terminalScrollbackLines, setTerminalScrollbackLines] = useState(8000);
  const [peerRuntimeVisibility, setPeerRuntimeVisibility] = useState<RuntimeVisibilityMode>("visible");
  const [petRuntimeVisibility, setPetRuntimeVisibility] = useState<RuntimeVisibilityMode>("hidden");
  const [obsBusy, setObsBusy] = useState(false);

  // Developer-mode debug views
  const [devActors, setDevActors] = useState<Actor[]>([]);
  const [debugSnapshot, setDebugSnapshot] = useState("");
  const [debugSnapshotErr, setDebugSnapshotErr] = useState("");
  const [debugSnapshotBusy, setDebugSnapshotBusy] = useState(false);
  const [runtimeVersion, setRuntimeVersion] = useState("");
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
    let cancelled = false;
    const loadWebAccessSession = async () => {
      try {
        const resp = await api.fetchWebAccessSession();
        if (cancelled) return;
        const session = resp.ok ? resp.result?.web_access_session ?? null : null;
        setWebAccessSession(session);
        const allowed = Boolean(session?.can_access_global_settings ?? !(session?.login_active ?? false));
        setCanAccessGlobalSettings(allowed);
        const allowGlobalScope = Boolean(allowed || session?.current_browser_signed_in);
        if (!allowGlobalScope && groupId) setScope("group");
      } catch {
        if (!cancelled) {
          setWebAccessSession(null);
          setCanAccessGlobalSettings(true);
        }
      }
    };
    void loadWebAccessSession();
    return () => {
      cancelled = true;
    };
  }, [isOpen, groupId]);

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
    setImWecomBotId("");
    setImWecomSecret("");
    setImWeixinAccountId("");
  };

  const loadIMStatus = useCallback(async (opts?: { resetFirst?: boolean }) => {
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
        setImBotTokenEnv(im.bot_token_env || im.bot_token || im.token_env || im.token || "");
        setImAppTokenEnv(im.app_token_env || im.app_token || "");
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
        setImDingtalkAppKey(im.dingtalk_app_key || im.dingtalk_app_key_env || "");
        setImDingtalkAppSecret(im.dingtalk_app_secret || im.dingtalk_app_secret_env || "");
        setImDingtalkRobotCode(im.dingtalk_robot_code || im.dingtalk_robot_code_env || "");
        setImWecomBotId(im.wecom_bot_id || "");
        setImWecomSecret(im.wecom_secret || "");
        setImWeixinAccountId(im.weixin_account_id || "");
      }
    } catch (e) {
      console.error("Failed to load IM status:", e);
    }
  }, [groupId]);

  useEffect(() => {
    if (!isOpen) return;
    loadIMStatus({ resetFirst: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Only load when the modal opens or groupId changes.
  }, [isOpen, groupId]);

  const toWeixinErrorStatus = useCallback((message: string): WeixinLoginStatus => ({
    status: "error",
    logged_in: false,
    account_id: "",
    qrcode_url: "",
    qr_ascii: "",
    error: String(message || "").trim(),
    running: false,
    pid: null,
    updated_at: new Date().toISOString(),
  }), []);

  useEffect(() => {
    if (!isOpen || !groupId || imPlatform !== "weixin") return;
    let cancelled = false;
    const loadWeixinStatus = async () => {
      try {
        const resp = await api.fetchWeixinLoginStatus(groupId);
        if (cancelled) return;
        if (resp.ok) {
          setWeixinLoginStatus(resp.result ?? null);
        } else {
          setWeixinLoginStatus(toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinStatusLoadFailed")));
        }
      } catch {
        if (!cancelled) {
          setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinStatusLoadFailed")));
        }
      }
    };
    void loadWeixinStatus();
    // Only poll while waiting for QR scan; stop once logged in or idle
    const needsPoll = weixinLoginStatus?.status === "waiting_scan";
    if (!needsPoll) return () => { cancelled = true; };
    const timer = window.setInterval(() => {
      void loadWeixinStatus();
    }, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isOpen, groupId, imPlatform, weixinLoginStatus?.status, t, toWeixinErrorStatus]);

  useEffect(() => {
    if (imPlatform !== "weixin") {
      weixinAutoStartRef.current = false;
      return;
    }
    if (!groupId) return;
    if (!weixinLoginStatus?.logged_in) return;
    if (!imStatus?.configured || String(imStatus.platform || "") !== "weixin") return;
    if (!imStatus.enabled) {
      weixinAutoStartRef.current = false;
      return;
    }
    if (imStatus.running) {
      weixinAutoStartRef.current = false;
      return;
    }
    if (weixinAutoStartRef.current) return;

    weixinAutoStartRef.current = true;
    void (async () => {
      setImBusy(true);
      try {
        const resp = await api.startIMBridge(groupId);
        if (resp.ok) {
          await loadIMStatus();
        }
      } catch (e) {
        console.error("Failed to auto-start weixin bridge:", e);
      } finally {
        setImBusy(false);
      }
    })();
  }, [groupId, imPlatform, imStatus, loadIMStatus, weixinLoginStatus]);

  useEffect(() => {
    if (isOpen && canAccessGlobalSettings === true) loadObservability();
  }, [isOpen, canAccessGlobalSettings]);

  useEffect(() => {
    if (isOpen && groupId) loadDevActors();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Only load when the modal opens or groupId changes.
  }, [isOpen, groupId]);

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

  // ============ Data Loading ============

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
          String(obs.runtime_visibility?.peer_runtime || "").trim().toLowerCase() === "hidden" ? "hidden" : "visible"
        );
        setPetRuntimeVisibility(
          String(obs.runtime_visibility?.pet_runtime || "").trim().toLowerCase() === "visible" ? "visible" : "hidden"
        );
      }
    } catch (e) {
      console.error("Failed to load observability settings:", e);
    }
  };

  const loadDevActors = async () => {
    if (!groupId) return;
    try {
      const resp = await api.fetchActors(groupId, false);
      if (resp.ok && resp.result?.actors) {
        const actors = Array.isArray(resp.result.actors) ? resp.result.actors : [];
        setDevActors(actors);
        if (!tailActorId && actors.length > 0) {
          setTailActorId(actors[0].id);
        }
      }
    } catch (e) {
      console.error("Failed to load developer actor list:", e);
    }
  };

  // ============ Handlers ============

  const handleSaveDeliverySettings = async () => {
    await onUpdateSettings({
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

  const handleResetAutomationSettingsDraft = () => {
    setNudgeSeconds(300);
    setReplyRequiredNudgeSeconds(300);
    setAttentionAckNudgeSeconds(600);
    setUnreadNudgeSeconds(900);
    setNudgeDigestMinIntervalSeconds(120);
    setNudgeMaxRepeatsPerObligation(3);
    setNudgeEscalateAfterRepeats(2);
    setIdleSeconds(0);
    setKeepaliveSeconds(120);
    setKeepaliveMax(3);
    setSilenceSeconds(0);
    setHelpNudgeIntervalSeconds(600);
    setHelpNudgeMinMessages(10);
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

    const ok = await copyTextToClipboard(payload);
    setToast(ok ? t("automation.copiedLines", { n }) : t("common:copyFailed"));
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
        setTailErr(resp.error?.message || t("automation.failedToLoadTranscript"));
      }
    } catch {
      setTailText("");
      setTailHint("");
      setTailErr(t("automation.failedToLoadTranscript"));
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
        setTailErr(resp.error?.message || t("automation.failedToClearTranscript"));
        return;
      }
      setTailText("");
      setTailHint("");
    } catch {
      setTailErr(t("automation.failedToClearTranscript"));
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
    wecomBotId: imWecomBotId,
    wecomSecret: imWecomSecret,
    weixinAccountId: imWeixinAccountId,
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
    setImWecomBotId(draft.wecomBotId);
    setImWecomSecret(draft.wecomSecret);
    setImWeixinAccountId(draft.weixinAccountId);
  };

  const getCurrentIMSaveRequest = () => ({
    groupId: String(groupId || ""),
    platform: imPlatform,
    ...getCurrentIMConfigDraft(),
  });

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
      setImWecomBotId("");
      setImWecomSecret("");
      setImWeixinAccountId("");
    }

    // 3. Set new platform
    setImPlatform(newPlatform);
  };

  const handleSaveIMConfig = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      const resp = await saveIMConfigDraft(getCurrentIMSaveRequest());
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
        setImWecomBotId("");
        setImWecomSecret("");
        setImWeixinAccountId("");
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
      const resp = await saveAndStartIMBridge(getCurrentIMSaveRequest());
      if (resp.ok) await loadIMStatus();
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

  const handleStartWeixinLogin = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      const saveResp = await saveIMConfigDraft(getCurrentIMSaveRequest());
      if (!saveResp.ok) {
        setWeixinLoginStatus(toWeixinErrorStatus(saveResp.error?.message || t("imBridge.weixinStartFailed")));
        return;
      }
      await loadIMStatus();
      weixinAutoStartRef.current = false;
      const resp = await api.startWeixinLogin(groupId);
      if (resp.ok) {
        setWeixinLoginStatus(resp.result ?? null);
      } else {
        setWeixinLoginStatus(toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinStartFailed")));
      }
    } catch (e) {
      setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinStartFailed")));
      console.error("Failed to start weixin login:", e);
    } finally {
      setImBusy(false);
    }
  };

  const handleLogoutWeixin = async () => {
    if (!groupId) return;
    setImBusy(true);
    try {
      weixinAutoStartRef.current = false;
      const resp = await api.logoutWeixin(groupId);
      if (resp.ok) {
        setWeixinLoginStatus(resp.result ?? null);
      } else {
        setWeixinLoginStatus(toWeixinErrorStatus(resp.error?.message || t("imBridge.weixinLogoutFailed")));
      }
    } catch (e) {
      setWeixinLoginStatus(toWeixinErrorStatus(t("imBridge.weixinLogoutFailed")));
      console.error("Failed to logout weixin:", e);
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
        peerRuntimeVisibility,
        petRuntimeVisibility,
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
          String(obs.runtime_visibility?.peer_runtime || "").trim().toLowerCase() === "hidden" ? "hidden" : "visible"
        );
        setPetRuntimeVisibility(
          String(obs.runtime_visibility?.pet_runtime || "").trim().toLowerCase() === "visible" ? "visible" : "hidden"
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

  const loadRuntimeInfo = async () => {
    setRuntimeInfoErr("");
    try {
      const resp = await api.fetchPing();
      if (!resp.ok) {
        setRuntimeVersion("");
        setDaemonVersion("");
        setRuntimeInfoErr(resp.error?.message || "Failed to load runtime info");
        return;
      }
      const result = resp.result || {};
      const daemon = result.daemon && typeof result.daemon === "object" && !Array.isArray(result.daemon)
        ? result.daemon as Record<string, unknown>
        : null;
      setRuntimeVersion(String(result.version || "").trim());
      setDaemonVersion(String(daemon?.version || "").trim());
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
      setLogErr(t("developer.imLogsRequireGroup"));
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

  // ============ Derived state (must be before early return to keep hooks stable) ============

  const globalSettingsEnabled = canAccessGlobalSettings === true;
  const currentBrowserSignedIn = Boolean(webAccessSession?.current_browser_signed_in);
  const globalScopeEnabled = globalSettingsEnabled || currentBrowserSignedIn;

  const globalTabs = useMemo<{ id: GlobalTabId; label: string }[]>(() => [
    ...(globalSettingsEnabled ? [
      { id: "capabilities" as const, label: t("tabs.capabilities") },
      { id: "actorProfiles" as const, label: t("tabs.actorProfiles") },
    ] : []),
    // Non-admin signed-in users see My Profiles; admin already has Actor Profiles covering all
    ...(currentBrowserSignedIn && !globalSettingsEnabled ? [{ id: "myProfiles" as const, label: t("tabs.myProfiles") }] : []),
    ...(globalSettingsEnabled ? [
      { id: "branding" as const, label: t("tabs.branding") },
      { id: "webAccess" as const, label: t("tabs.webAccess") },
      { id: "webModels" as const, label: t("tabs.webModels", { defaultValue: "ChatGPT Web Model" }) },
      { id: "developer" as const, label: t("tabs.developer") },
    ] : []),
  ], [globalSettingsEnabled, currentBrowserSignedIn, t]);

  useEffect(() => {
    if (scope !== "global") return;
    if (!globalTabs.length) return;
    if (!globalTabs.some((tab) => tab.id === globalTab)) {
      setGlobalTab(globalTabs[0].id);
    }
  }, [globalTab, globalTabs, scope]);

  const groupTabs: { id: GroupTabId; label: string }[] = [
    { id: "guidance", label: t("tabs.guidance") },
    { id: "assistants", label: t("tabs.assistants") },
    { id: "automation", label: t("tabs.automation") },
    { id: "delivery", label: t("tabs.delivery") },
    { id: "space", label: t("tabs.space") },
    { id: "messaging", label: t("tabs.messaging") },
    { id: "im", label: t("tabs.im") },
    { id: "transcript", label: t("tabs.transcript") },
    { id: "blueprint", label: t("tabs.blueprint") },
  ];
  const tabs = scope === "group" ? groupTabs : (globalScopeEnabled ? globalTabs : []);
  const activeTab = scope === "group" ? groupTab : globalTab;
  const setActiveTab = (tab: GroupTabId | GlobalTabId) => {
    if (scope === "group") setGroupTab(tab as GroupTabId);
    else setGlobalTab(tab as GlobalTabId);
  };

  useEffect(() => {
    const el = contentScrollRef.current;
    if (!isOpen || !el) return;
    requestAnimationFrame(() => {
      el.scrollTo({ top: 0, behavior: "auto" });
    });
  }, [isOpen, scope, activeTab]);

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

  return (
    <ModalFrame
      isDark={isDark}
      onClose={onClose}
      titleId="settings-modal-title"
      title={(
        <div className="min-w-0">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
            Workspace Settings
          </div>
          <h2 className="mt-1 truncate text-[1.15rem] font-semibold text-[var(--color-text-primary)]">
            {t("title")}
          </h2>
          <div className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
            {scope === "group"
              ? t("navigation.groupScopeContent", { scopeRoot: scopeRootUrl || groupId || "—" })
              : (globalScopeEnabled ? t("navigation.globalScopeContent") : t("navigation.globalLockedContent"))}
          </div>
        </div>
      )}
      closeAriaLabel={t("closeAriaLabel")}
      panelClassName="w-full h-full sm:h-[min(90dvh,920px)] sm:max-w-[min(1280px,calc(100vw-2rem))] sm:max-h-[90dvh]"
      headerActions={(
        <div className="hidden sm:flex items-center gap-2">
          <span className="rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
            {scope === "group" ? t("navigation.thisGroup") : t("navigation.global")}
          </span>
          {groupDoc?.title && scope === "group" ? (
            <span className="max-w-[18rem] truncate rounded-full border border-[var(--glass-border-subtle)] bg-transparent px-3 py-1 text-[11px] font-medium text-[var(--color-text-tertiary)]">
              {groupDoc.title}
            </span>
          ) : null}
        </div>
      )}
      modalRef={modalRef}
    >
      <div className="min-h-0 flex-1 flex flex-col sm:flex-row overflow-hidden">
        <SettingsNavigation
          isDark={isDark}
          groupId={groupId}
          scope={scope}
          scopeRootUrl={scopeRootUrl}
          globalEnabled={globalScopeEnabled}
          tabs={tabs}
          activeTab={activeTab}
          onScopeChange={setScope}
          onTabChange={(tab) => setActiveTab(tab as GroupTabId | GlobalTabId)}
        />

        {/* Main Content Area */}
        <div
          ref={contentScrollRef}
          className={`min-h-0 flex-1 overflow-y-auto scrollbar-subtle flex flex-col [scrollbar-gutter:stable] ${
            isDark
              ? "bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),transparent_32%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))]"
              : "bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.96),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgb(255,255,255),rgb(246,248,251))]"
          }`}
        >
          <div className="p-4 pb-6 sm:p-5 lg:p-6 sm:pb-7 space-y-4 lg:space-y-5">
            {scope === "global" && !globalSettingsEnabled && !currentBrowserSignedIn ? (
              <div className={`rounded-xl border p-6 ${isDark ? "border-amber-700/40 bg-amber-900/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"}`}>
                <div className="text-sm font-semibold">{t("navigation.globalLockedTitle")}</div>
                <div className="mt-2 text-sm leading-6">{t("navigation.globalLockedContent")}</div>
                {groupId ? (
                  <button
                    type="button"
                    onClick={() => setScope("group")}
                    className={`mt-4 px-3 py-2 rounded-lg text-xs ${isDark ? "bg-slate-800 text-slate-100 hover:bg-slate-700" : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"}`}
                  >
                    {t("navigation.thisGroup")}
                  </button>
                ) : null}
              </div>
            ) : !tabs.some((tab) => tab.id === activeTab) ? null : (
              <Suspense fallback={<SettingsTabFallback isDark={isDark} />}>
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
                  onResetPolicies={handleResetAutomationSettingsDraft}
                />
              )}

              {activeTab === "delivery" && (
                <DeliveryTab
                  isDark={isDark}
                  busy={busy}
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
                  imWecomBotId={imWecomBotId}
                  setImWecomBotId={setImWecomBotId}
                  imWecomSecret={imWecomSecret}
                  setImWecomSecret={setImWecomSecret}
                  imWeixinAccountId={imWeixinAccountId}
                  setImWeixinAccountId={setImWeixinAccountId}
                  weixinLoginStatus={weixinLoginStatus}
                  onStartWeixinLogin={handleStartWeixinLogin}
                  onLogoutWeixin={handleLogoutWeixin}
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

              {activeTab === "assistants" && (
                <AssistantsTab
                  isDark={isDark}
                  groupId={groupId}
                  isActive={scope === "group" && activeTab === "assistants"}
                  petEnabled={Boolean(settings?.desktop_pet_enabled)}
                  busy={busy}
                  onUpdatePetEnabled={(enabled) => onUpdateSettings({ desktop_pet_enabled: enabled })}
                />
              )}

              {activeTab === "space" && (
                <GroupSpaceTab
                  isDark={isDark}
                  groupId={groupId}
                  isActive={scope === "group" && activeTab === "space"}
                />
              )}

              {activeTab === "blueprint" && <BlueprintTab isDark={isDark} groupId={groupId} groupTitle={groupDoc?.title || ""} />}

              {activeTab === "capabilities" && (
                <CapabilitiesTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "capabilities"}
                  groupId={groupId}
                />
              )}

              {activeTab === "actorProfiles" && (
                <ActorProfilesTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "actorProfiles"}
                  scope="global"
                />
              )}

              {activeTab === "myProfiles" && (
                <ActorProfilesTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "myProfiles"}
                  scope="my"
                />
              )}

              {activeTab === "branding" && (
                <BrandingTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "branding"}
                />
              )}

              {activeTab === "webAccess" && (
                <WebAccessTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "webAccess"}
                />
              )}

              {activeTab === "webModels" && (
                <WebModelConnectorsTab
                  isDark={isDark}
                  isActive={scope === "global" && activeTab === "webModels"}
                  currentGroupId={groupId}
                  onOpenWebAccess={() => setGlobalTab("webAccess")}
                />
              )}


              {activeTab === "developer" && (
                <DeveloperTab
                  isDark={isDark}
                  groupId={groupId}
                  runtimeVersion={runtimeVersion}
                  daemonVersion={daemonVersion}
                  runtimeInfoErr={runtimeInfoErr}
                  developerMode={developerMode}
                  setDeveloperMode={setDeveloperMode}
                  logLevel={logLevel}
                  setLogLevel={setLogLevel}
                  terminalBacklogMiB={terminalBacklogMiB}
                  setTerminalBacklogMiB={setTerminalBacklogMiB}
                  terminalScrollbackLines={terminalScrollbackLines}
                  setTerminalScrollbackLines={setTerminalScrollbackLines}
                  peerRuntimeVisibility={peerRuntimeVisibility}
                  setPeerRuntimeVisibility={setPeerRuntimeVisibility}
                  petRuntimeVisibility={petRuntimeVisibility}
                  setPetRuntimeVisibility={setPetRuntimeVisibility}
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
              </Suspense>
            )}
          </div>
        </div>
      </div>
    </ModalFrame>
  );
}
