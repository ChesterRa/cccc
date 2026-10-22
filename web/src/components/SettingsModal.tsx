// SettingsModal renders the settings modal.
import { GroupConnectionsPanel } from "../features/connect/GroupConnectionsControl";
import { lazy, Suspense, useState, useEffect, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { GroupDoc, GroupSettings, WebAccessSession } from "../types";
import * as api from "../services/api";
import { useModalStore } from "../stores";
import { SettingsScope, GroupTabId, GlobalTabId } from "./modals/settings/types";
import {
  readSettingsLastLocation,
  writeSettingsLastLocation,
} from "./modals/settings/settingsLastLocation";
import { ModalFrame } from "./modals/ModalFrame";
import { SettingsNavigation } from "./modals/settings/SettingsNavigation";
import { useSettingsDiagnostics } from "./modals/settings/useSettingsDiagnostics";
import { useSettingsIMBridge } from "./modals/settings/useSettingsIMBridge";
import { useModalA11y } from "../hooks/useModalA11y";

const AutomationTab = lazy(() =>
  import("./modals/settings/AutomationTab").then((module) => ({ default: module.AutomationTab })),
);
const DeliveryTab = lazy(() =>
  import("./modals/settings/DeliveryTab").then((module) => ({ default: module.DeliveryTab })),
);
const MessagingTab = lazy(() =>
  import("./modals/settings/MessagingTab").then((module) => ({ default: module.MessagingTab })),
);
const IMBridgeTab = lazy(() =>
  import("./modals/settings/IMBridgeTab").then((module) => ({ default: module.IMBridgeTab })),
);
const TranscriptTab = lazy(() =>
  import("./modals/settings/TranscriptTab").then((module) => ({ default: module.TranscriptTab })),
);
const GuidanceTab = lazy(() =>
  import("./modals/settings/GuidanceTab").then((module) => ({ default: module.GuidanceTab })),
);
const VoiceSettingsTab = lazy(() =>
  import("./modals/settings/VoiceSettingsTab").then((module) => ({
    default: module.VoiceSettingsTab,
  })),
);
const GroupSpaceTab = lazy(() =>
  import("./modals/settings/GroupSpaceTab").then((module) => ({ default: module.GroupSpaceTab })),
);
const CopyGroupsTab = lazy(() =>
  import("./modals/settings/CopyGroupsTab").then((module) => ({ default: module.CopyGroupsTab })),
);
const CapabilitiesTab = lazy(() =>
  import("./modals/settings/CapabilitiesTab").then((module) => ({
    default: module.CapabilitiesTab,
  })),
);
const ActorProfilesTab = lazy(() =>
  import("./modals/settings/ActorProfilesTab").then((module) => ({
    default: module.ActorProfilesTab,
  })),
);
const BrandingTab = lazy(() =>
  import("./modals/settings/BrandingTab").then((module) => ({ default: module.BrandingTab })),
);
const AccountTab = lazy(() =>
  import("./modals/settings/AccountTab").then((module) => ({ default: module.AccountTab })),
);
const WebAccessTab = lazy(() =>
  import("./modals/settings/WebAccessTab").then((module) => ({ default: module.WebAccessTab })),
);
const WebModelConnectorsTab = lazy(() =>
  import("./modals/settings/WebModelConnectorsTab").then((module) => ({ default: module.default })),
);
const DeveloperTab = lazy(() =>
  import("./modals/settings/DeveloperTab").then((module) => ({ default: module.DeveloperTab })),
);

interface SettingsModalProps {
  codexVoice: import("../features/codexVoice/useCodexVoiceShell").CodexVoiceShellState;
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

function SettingsTabFallback() {
  return (
    <div className="rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-5 text-sm text-[var(--color-text-secondary)]">
      Loading...
    </div>
  );
}

export function SettingsModal({
  codexVoice,
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
  const [voiceSection, setVoiceSection] = useState<"secretary" | "realtime">("secretary");
  const [voiceAudioRequest, setVoiceAudioRequest] = useState(0);
  const [voiceFeatureRequest, setVoiceFeatureRequest] = useState(0);
  const [voiceAnalystRequest, setVoiceAnalystRequest] = useState(0);
  const voiceBeforeLeave = useRef<() => boolean>(() => true);
  const requestClose = () => {
    if (voiceBeforeLeave.current()) onClose();
  };
  const initialVoiceFocus = useRef<HTMLElement | null>(null);
  const { modalRef } = useModalA11y(isOpen, requestClose, { initialFocusRef: initialVoiceFocus });
  const [initialLocation] = useState(() => readSettingsLastLocation(Boolean(groupId)));
  const [scope, setScope] = useState<SettingsScope>(() => initialLocation.scope);
  const [groupTab, setGroupTab] = useState<GroupTabId>(() => initialLocation.groupTab);
  const [globalTab, setGlobalTab] = useState<GlobalTabId>(() => initialLocation.globalTab);
  const [accountReturnToWebAccess, setAccountReturnToWebAccess] = useState(false);
  const [focusReachOnOpen, setFocusReachOnOpen] = useState(false);
  const [canAccessGlobalSettings, setCanAccessGlobalSettings] = useState<boolean | null>(null);
  const [webAccessSession, setWebAccessSession] = useState<WebAccessSession | null>(null);
  const [webModelProvider, setWebModelProvider] = useState<api.WebModelView>("chatgpt_web");
  const [webModelFocusTarget, setWebModelFocusTarget] = useState<{
    section: "login" | "connector";
    nonce: number;
  }>();
  const settingsTarget = useModalStore((state) => state.settingsTarget);
  const clearSettingsTarget = useModalStore((state) => state.clearSettingsTarget);
  useEffect(() => {
    if (!isOpen) setWebModelFocusTarget(undefined);
  }, [isOpen]);

  // Automation + delivery settings state
  const [mailNoticeAfterSeconds, setMailNoticeAfterSeconds] = useState(1800);
  const [mailNoticeIdleAfterSeconds, setMailNoticeIdleAfterSeconds] = useState(60);
  const [replyNoticeAfterSeconds, setReplyNoticeAfterSeconds] = useState(900);
  const [idleSeconds, setIdleSeconds] = useState(0);
  const [keepaliveSeconds, setKeepaliveSeconds] = useState(120);
  const [keepaliveMax, setKeepaliveMax] = useState(3);
  const [silenceSeconds, setSilenceSeconds] = useState(0);
  const [helpNudgeIntervalSeconds, setHelpNudgeIntervalSeconds] = useState(600);
  const [helpNudgeMinMessages, setHelpNudgeMinMessages] = useState(10);

  // Messaging policy
  const [defaultSendTo, setDefaultSendTo] = useState<"foreman" | "broadcast">("foreman");

  // Terminal transcript (group-scoped policy)
  const [terminalVisibility, setTerminalVisibility] = useState<"off" | "foreman" | "all">(
    "foreman",
  );
  const [terminalNotifyTail, setTerminalNotifyTail] = useState(false);
  const [terminalNotifyLines, setTerminalNotifyLines] = useState(20);

  const {
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
  } = useSettingsDiagnostics({
    isOpen,
    groupId,
    scope,
    groupTab,
    globalTab,
    hasSettingsTarget: Boolean(settingsTarget),
    canAccessGlobalSettings,
    onRegistryChanged,
    t,
  });

  // IM Bridge state
  const {
    imStatus,
    imConfigError,
    imPlatform,
    handlePlatformChange,
    imBotTokenEnv,
    setImBotTokenEnv,
    imAppTokenEnv,
    setImAppTokenEnv,
    imMattermostUrl,
    setImMattermostUrl,
    imFeishuAppId,
    setImFeishuAppId,
    imFeishuAppSecret,
    setImFeishuAppSecret,
    imFeishuDomain,
    setImFeishuDomain,
    imDingtalkAppKey,
    setImDingtalkAppKey,
    imDingtalkAppSecret,
    setImDingtalkAppSecret,
    imDingtalkRobotCode,
    setImDingtalkRobotCode,
    imWecomBotId,
    setImWecomBotId,
    imWecomSecret,
    setImWecomSecret,
    imWeixinAccountId,
    setImWeixinAccountId,
    weixinLoginStatus,
    handleStartWeixinLogin,
    handleVerifyWeixin,
    handleLogoutWeixin,
    imBusy,
    handleSaveIMConfig,
    handleRemoveIMConfig,
    handleStartBridge,
    handleStopBridge,
  } = useSettingsIMBridge({
    isOpen,
    groupId,
    scope,
    groupTab,
    hasSettingsTarget: Boolean(settingsTarget),
    t,
  });

  const contentScrollRef = useRef<HTMLDivElement | null>(null);

  // ============ Effects ============

  const previousSettings = useRef<{ groupId: string | undefined; value: GroupSettings } | null>(
    null,
  );
  useEffect(() => {
    if (!isOpen || !settings) {
      previousSettings.current = null;
      return;
    }
    const previous =
      previousSettings.current && previousSettings.current.groupId === groupId
        ? previousSettings.current.value
        : null;
    // Refresh clean fields; another section's save must not overwrite a local draft.
    const sync = <T,>(current: T, before: T | undefined, next: T): T =>
      previous === null || Object.is(current, before) ? next : current;
    setMailNoticeAfterSeconds((current) =>
      sync(
        current,
        previous?.mail_notice_after_seconds ?? 1800,
        settings.mail_notice_after_seconds ?? 1800,
      ),
    );
    setMailNoticeIdleAfterSeconds((current) =>
      sync(
        current,
        previous?.mail_notice_idle_after_seconds ?? 60,
        settings.mail_notice_idle_after_seconds ?? 60,
      ),
    );
    setReplyNoticeAfterSeconds((current) =>
      sync(
        current,
        previous?.reply_notice_after_seconds ?? 900,
        settings.reply_notice_after_seconds ?? 900,
      ),
    );
    setIdleSeconds((current) =>
      sync(current, previous?.actor_idle_timeout_seconds, settings.actor_idle_timeout_seconds),
    );
    setKeepaliveSeconds((current) =>
      sync(current, previous?.keepalive_delay_seconds, settings.keepalive_delay_seconds),
    );
    setKeepaliveMax((current) =>
      sync(current, previous?.keepalive_max_per_actor ?? 3, settings.keepalive_max_per_actor ?? 3),
    );
    setSilenceSeconds((current) =>
      sync(current, previous?.silence_timeout_seconds, settings.silence_timeout_seconds),
    );
    setHelpNudgeIntervalSeconds((current) =>
      sync(
        current,
        previous?.help_nudge_interval_seconds ?? 600,
        settings.help_nudge_interval_seconds ?? 600,
      ),
    );
    setHelpNudgeMinMessages((current) =>
      sync(
        current,
        previous?.help_nudge_min_messages ?? 10,
        settings.help_nudge_min_messages ?? 10,
      ),
    );
    setDefaultSendTo((current) =>
      sync(current, previous?.default_send_to || "foreman", settings.default_send_to || "foreman"),
    );
    setTerminalVisibility((current) =>
      sync(
        current,
        previous?.terminal_transcript_visibility || "foreman",
        settings.terminal_transcript_visibility || "foreman",
      ),
    );
    setTerminalNotifyTail((current) =>
      sync(
        current,
        Boolean(previous?.terminal_transcript_notify_tail),
        Boolean(settings.terminal_transcript_notify_tail),
      ),
    );
    setTerminalNotifyLines((current) =>
      sync(
        current,
        Number(previous?.terminal_transcript_notify_lines || 20),
        Number(settings.terminal_transcript_notify_lines || 20),
      ),
    );
    previousSettings.current = { groupId, value: settings };
  }, [isOpen, groupId, settings]);

  useEffect(() => {
    if (!isOpen) return;
    setScope(groupId ? "group" : "global");
  }, [isOpen, groupId]);

  useEffect(() => {
    if (isOpen) return;
    initialVoiceFocus.current = null;
    setVoiceAudioRequest(0);
    setVoiceFeatureRequest(0);
    setAccountReturnToWebAccess(false);
    setFocusReachOnOpen(false);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    const loadWebAccessSession = async () => {
      try {
        const resp = await api.fetchWebAccessSession();
        if (cancelled) return;
        const session = resp.ok ? (resp.result?.web_access_session ?? null) : null;
        setWebAccessSession(session);
        const allowed = Boolean(
          session?.can_access_global_settings ?? !(session?.login_active ?? false),
        );
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

  // ============ Handlers ============

  const [saveFeedback, setSaveFeedback] = useState<{
    context: string;
    error: boolean;
    message: string;
  } | null>(null);
  const saveContext = JSON.stringify([
    isOpen,
    groupId,
    scope,
    scope === "group" ? groupTab : globalTab,
    mailNoticeAfterSeconds,
    mailNoticeIdleAfterSeconds,
    replyNoticeAfterSeconds,
    idleSeconds,
    keepaliveSeconds,
    keepaliveMax,
    silenceSeconds,
    helpNudgeIntervalSeconds,
    helpNudgeMinMessages,
    defaultSendTo,
    terminalVisibility,
    terminalNotifyTail,
    terminalNotifyLines,
  ]);
  const saveContextRef = useRef(saveContext);
  saveContextRef.current = saveContext;
  useEffect(
    () => () => {
      saveContextRef.current = "";
    },
    [],
  );
  const saveGroupSettings = async (patch: Partial<GroupSettings>) => {
    const context = saveContextRef.current;
    setSaveFeedback(null);
    try {
      const result = await onUpdateSettings(patch);
      if (saveContextRef.current === context) {
        setSaveFeedback({
          context,
          error: result === false,
          message: t(result === false ? "saveFeedback.failed" : "saveFeedback.saved"),
        });
      }
    } catch (error) {
      if (saveContextRef.current === context) {
        setSaveFeedback({
          context,
          error: true,
          message: error instanceof Error ? error.message : t("saveFeedback.failed"),
        });
      }
    }
  };

  const handleSaveDeliverySettings = async () => {
    await saveGroupSettings({
      mail_notice_after_seconds: mailNoticeAfterSeconds,
      mail_notice_idle_after_seconds: mailNoticeIdleAfterSeconds,
      reply_notice_after_seconds: replyNoticeAfterSeconds,
    });
  };

  const handleSaveAutomationSettings = async () => {
    await saveGroupSettings({
      actor_idle_timeout_seconds: idleSeconds,
      keepalive_delay_seconds: keepaliveSeconds,
      keepalive_max_per_actor: keepaliveMax,
      silence_timeout_seconds: silenceSeconds,
      help_nudge_interval_seconds: helpNudgeIntervalSeconds,
      help_nudge_min_messages: helpNudgeMinMessages,
    });
  };

  const handleResetAutomationSettingsDraft = () => {
    setIdleSeconds(0);
    setKeepaliveSeconds(120);
    setKeepaliveMax(3);
    setSilenceSeconds(0);
    setHelpNudgeIntervalSeconds(600);
    setHelpNudgeMinMessages(10);
  };

  const handleSaveTranscriptSettings = async () => {
    await saveGroupSettings({
      terminal_transcript_visibility: terminalVisibility,
      terminal_transcript_notify_tail: terminalNotifyTail,
      terminal_transcript_notify_lines: terminalNotifyLines,
    });
  };

  const handleSaveMessagingSettings = async () => {
    await saveGroupSettings({ default_send_to: defaultSendTo });
  };

  // ============ Derived state (must be before early return to keep hooks stable) ============

  const globalSettingsEnabled = canAccessGlobalSettings === true;
  const currentBrowserSignedIn = Boolean(webAccessSession?.current_browser_signed_in);
  const globalScopeEnabled = globalSettingsEnabled || currentBrowserSignedIn;

  const globalTabs = useMemo<{ id: GlobalTabId; label: string }[]>(
    () => [
      ...(globalSettingsEnabled
        ? [
            { id: "account" as const, label: t("tabs.account") },
            { id: "capabilities" as const, label: t("tabs.capabilities") },
            { id: "actorProfiles" as const, label: t("tabs.actorProfiles") },
            { id: "voice" as const, label: t("tabs.voice") },
          ]
        : []),
      // Non-admin signed-in users see My Profiles; admin already has Actor Profiles covering all
      ...(currentBrowserSignedIn && !globalSettingsEnabled
        ? [{ id: "myProfiles" as const, label: t("tabs.myProfiles") }]
        : []),
      ...(globalSettingsEnabled
        ? [
            { id: "branding" as const, label: t("tabs.branding") },
            { id: "webAccess" as const, label: t("tabs.webAccess") },
            {
              id: "webModels" as const,
              label: t("tabs.webModels", { defaultValue: "ChatGPT Web Model" }),
            },
            { id: "developer" as const, label: t("tabs.developer") },
          ]
        : []),
    ],
    [globalSettingsEnabled, currentBrowserSignedIn, t],
  );

  useEffect(() => {
    if (scope !== "global") return;
    if (!globalTabs.length) return;
    if (!globalTabs.some((tab) => tab.id === globalTab)) {
      setGlobalTab(globalTabs[0].id);
    }
  }, [globalTab, globalTabs, scope]);

  useEffect(() => {
    if (!isOpen || !settingsTarget) return;
    if (!voiceBeforeLeave.current()) {
      clearSettingsTarget();
      return;
    }
    const nextScope =
      settingsTarget.scope === "global"
        ? "global"
        : settingsTarget.scope === "group"
          ? "group"
          : "";
    const nextTab = String(settingsTarget.tab || "").trim();
    if (nextScope === "global") {
      setScope("global");
      setAccountReturnToWebAccess(false);
      setFocusReachOnOpen(false);
      if (nextTab) setGlobalTab(nextTab as GlobalTabId);
    } else if (nextScope === "group") {
      setScope("group");
      setAccountReturnToWebAccess(false);
      setFocusReachOnOpen(false);
      if (nextTab) setGroupTab(nextTab as GroupTabId);
    }
    if (settingsTarget.voiceSection) setVoiceSection(settingsTarget.voiceSection);
    setVoiceAudioRequest(settingsTarget.voiceAudio ? settingsTarget.nonce : 0);
    setVoiceAnalystRequest(settingsTarget.voiceAnalyst ? settingsTarget.nonce : 0);
    setVoiceFeatureRequest(
      settingsTarget.voiceSection && !settingsTarget.voiceAudio ? settingsTarget.nonce : 0,
    );
    if (settingsTarget.webModelProvider) setWebModelProvider(settingsTarget.webModelProvider);
    setWebModelFocusTarget(
      settingsTarget.webModelSection
        ? { section: settingsTarget.webModelSection, nonce: settingsTarget.nonce }
        : undefined,
    );
    clearSettingsTarget();
  }, [clearSettingsTarget, isOpen, settingsTarget]);

  const groupTabs: { id: GroupTabId; label: string }[] = [
    { id: "guidance", label: t("tabs.guidance") },
    { id: "automation", label: t("tabs.automation") },
    { id: "delivery", label: t("tabs.delivery") },
    { id: "space", label: t("tabs.space") },
    { id: "messaging", label: t("tabs.messaging") },
    { id: "im", label: t("tabs.im") },
    ...(globalSettingsEnabled
      ? [{ id: "connections" as const, label: t("layout:groupConnections.title") }]
      : []),
    { id: "transcript", label: t("tabs.transcript") },
    { id: "copyGroups", label: t("tabs.copyGroups") },
  ];
  const tabs = scope === "group" ? groupTabs : globalScopeEnabled ? globalTabs : [];
  const activeTab = scope === "group" ? groupTab : globalTab;
  const setActiveTab = (tab: GroupTabId | GlobalTabId) => {
    setAccountReturnToWebAccess(false);
    setFocusReachOnOpen(false);
    if (scope === "group") setGroupTab(tab as GroupTabId);
    else setGlobalTab(tab as GlobalTabId);
  };

  const openAccountFromWebAccess = () => {
    setAccountReturnToWebAccess(true);
    setFocusReachOnOpen(false);
    setGlobalTab("account");
  };

  const openWebAccessFromAccount = () => {
    setAccountReturnToWebAccess(false);
    setFocusReachOnOpen(true);
    setGlobalTab("webAccess");
  };

  useEffect(() => {
    if (!isOpen) return;
    writeSettingsLastLocation({ scope, groupTab, globalTab });
  }, [globalTab, groupTab, isOpen, scope]);

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
    const active = scopes.find(
      (s) => String(s?.scope_key || "") === activeKey && String(s?.url || "").trim(),
    );
    const first = scopes.find((s) => String(s?.url || "").trim());
    return String((active || first)?.url || "").trim();
  })();

  return (
    <ModalFrame
      isDark={isDark}
      onClose={requestClose}
      titleId="settings-modal-title"
      surface="solid"
      title={
        <h2 className="truncate text-lg font-semibold text-[var(--color-text-primary)]">
          {t("title")}
        </h2>
      }
      closeAriaLabel={t("closeAriaLabel")}
      panelClassName="w-full h-full sm:h-[min(90dvh,920px)] sm:max-w-[min(1280px,calc(100vw-2rem))] sm:max-h-[90dvh]"
      modalRef={modalRef}
    >
      <div className="min-h-0 flex-1 flex flex-col sm:flex-row overflow-hidden">
        <SettingsNavigation
          isDark={isDark}
          groupId={groupId}
          groupTitle={groupDoc?.title}
          scope={scope}
          scopeRootUrl={scopeRootUrl}
          globalEnabled={globalScopeEnabled}
          tabs={tabs}
          activeTab={activeTab}
          onScopeChange={(nextScope) => {
            if (nextScope !== scope && !voiceBeforeLeave.current()) return;
            setAccountReturnToWebAccess(false);
            setFocusReachOnOpen(false);
            setScope(nextScope);
          }}
          onTabChange={(tab) => {
            if (tab !== activeTab && !voiceBeforeLeave.current()) return;
            setActiveTab(tab as GroupTabId | GlobalTabId);
          }}
        />

        {/* Main Content Area */}
        <div
          ref={contentScrollRef}
          className="min-h-0 min-w-0 flex-1 overflow-y-auto scrollbar-subtle flex flex-col [scrollbar-gutter:stable] bg-[var(--color-bg-primary)]"
        >
          <div className="px-4 pt-5 pb-4 sm:px-8 sm:pt-7 lg:px-10 space-y-6">
            {scope === "global" && !globalSettingsEnabled && !currentBrowserSignedIn ? (
              <div
                className={`rounded-xl border p-6 ${isDark ? "border-amber-700/40 bg-amber-900/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"}`}
              >
                <div className="text-sm font-semibold">{t("navigation.globalLockedTitle")}</div>
                <div className="mt-2 text-sm leading-6">{t("navigation.globalLockedContent")}</div>
                {groupId ? (
                  <button
                    type="button"
                    onClick={() => setScope("group")}
                    className="mt-4 px-3 py-2 rounded-lg text-xs font-medium border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)] transition-all"
                  >
                    {t("navigation.thisGroup")}
                  </button>
                ) : null}
              </div>
            ) : !tabs.some((tab) => tab.id === activeTab) ? null : (
              <Suspense fallback={<SettingsTabFallback />}>
                {scope === "group" &&
                  activeTab === "connections" &&
                  groupId &&
                  globalSettingsEnabled && (
                    <section className="space-y-4">
                      <div>
                        <h3 className="text-base font-semibold">
                          {t("layout:groupConnections.title")} ·{" "}
                          {groupDoc?.group_id === groupId ? groupDoc.title || groupId : groupId}
                        </h3>
                        <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
                          {t("layout:groupConnections.description")}
                        </p>
                      </div>
                      <GroupConnectionsPanel
                        key={groupId}
                        groupId={groupId}
                        groupTitle={
                          groupDoc?.group_id === groupId ? groupDoc.title || groupId : groupId
                        }
                        onOpenAccount={() => {
                          setAccountReturnToWebAccess(false);
                          setFocusReachOnOpen(false);
                          setScope("global");
                          setGlobalTab("account");
                        }}
                      />
                    </section>
                  )}
                {activeTab === "automation" && (
                  <AutomationTab
                    isDark={isDark}
                    groupId={groupId}
                    devActors={devActors}
                    busy={busy}
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
                    mailNoticeAfterSeconds={mailNoticeAfterSeconds}
                    setMailNoticeAfterSeconds={setMailNoticeAfterSeconds}
                    mailNoticeIdleAfterSeconds={mailNoticeIdleAfterSeconds}
                    setMailNoticeIdleAfterSeconds={setMailNoticeIdleAfterSeconds}
                    replyNoticeAfterSeconds={replyNoticeAfterSeconds}
                    setReplyNoticeAfterSeconds={setReplyNoticeAfterSeconds}
                    onSave={handleSaveDeliverySettings}
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
                    imConfigError={imConfigError}
                    imPlatform={imPlatform}
                    onPlatformChange={handlePlatformChange}
                    imBotTokenEnv={imBotTokenEnv}
                    setImBotTokenEnv={setImBotTokenEnv}
                    imAppTokenEnv={imAppTokenEnv}
                    setImAppTokenEnv={setImAppTokenEnv}
                    imMattermostUrl={imMattermostUrl}
                    setImMattermostUrl={setImMattermostUrl}
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
                    onVerifyWeixin={handleVerifyWeixin}
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

                {activeTab === "space" && (
                  <GroupSpaceTab
                    isDark={isDark}
                    groupId={groupId}
                    isActive={scope === "group" && activeTab === "space"}
                  />
                )}

                {activeTab === "copyGroups" && (
                  <CopyGroupsTab
                    isDark={isDark}
                    groupId={groupId}
                    groupTitle={groupDoc?.title || ""}
                  />
                )}

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

                {activeTab === "voice" && scope === "global" && globalSettingsEnabled && (
                  <VoiceSettingsTab
                    voice={codexVoice}
                    section={voiceSection}
                    audioRequest={voiceAudioRequest}
                    featureRequest={voiceFeatureRequest}
                    // A fresh target wins on the first render after reopening;
                    // the persisted request is updated by the parent effect.
                    analystRequest={
                      settingsTarget
                        ? settingsTarget.voiceAnalyst
                          ? settingsTarget.nonce
                          : 0
                        : voiceAnalystRequest
                    }
                    initialFocusRef={initialVoiceFocus}
                    onSectionChange={setVoiceSection}
                    isDark={isDark}
                    isActive={isOpen}
                    onBeforeLeaveChange={(guard) => {
                      voiceBeforeLeave.current = guard;
                    }}
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

                {activeTab === "account" && (
                  <AccountTab
                    isDark={isDark}
                    isActive={scope === "global" && activeTab === "account"}
                    returnToWebAccess={accountReturnToWebAccess}
                    onOpenWebAccess={openWebAccessFromAccount}
                  />
                )}

                {activeTab === "webAccess" && (
                  <WebAccessTab
                    isDark={isDark}
                    isActive={scope === "global" && activeTab === "webAccess"}
                    focusReach={focusReachOnOpen}
                    onOpenAccount={openAccountFromWebAccess}
                  />
                )}

                {activeTab === "webModels" && (
                  <WebModelConnectorsTab
                    focusTarget={webModelFocusTarget}
                    provider={webModelProvider}
                    onProviderChange={setWebModelProvider}
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
                    runtimeBuildInfo={runtimeBuildInfo}
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
                    assistantRuntimeVisibility={assistantRuntimeVisibility}
                    setAssistantRuntimeVisibility={setAssistantRuntimeVisibility}
                    obsBusy={obsBusy}
                    onSaveObservability={handleSaveObservability}
                    debugSnapshot={debugSnapshot}
                    debugSnapshotErr={debugSnapshotErr}
                    debugSnapshotBusy={debugSnapshotBusy}
                    onLoadDebugSnapshot={loadDebugSnapshot}
                    onClearDebugSnapshot={clearDebugSnapshot}
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
            {saveFeedback?.context === saveContext && (
              <p
                role={saveFeedback.error ? "alert" : "status"}
                className={`text-sm ${saveFeedback.error ? "text-rose-700 dark:text-rose-300" : "text-[var(--color-accent-success)]"}`}
              >
                {saveFeedback.message}
              </p>
            )}
          </div>
        </div>
      </div>
    </ModalFrame>
  );
}
