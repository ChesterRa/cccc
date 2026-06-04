import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTranslation } from "react-i18next";
import { Actor, AgentState, HeadlessPreviewSession, HeadlessStreamEvent, StreamingActivity, RUNTIME_INFO } from "../types";
import { useActorDisplayState } from "../hooks/useActorDisplayState";
import { getTerminalTheme } from "../hooks/useTheme";
import { classNames } from "../utils/classNames";
import { formatFullTime, formatTime } from "../utils/time";
import { useGroupStore, useObservabilityStore, useTerminalSignalsStore } from "../stores";
import { HeadlessRuntimePanel } from "./headless/HeadlessRuntimePanel";
import { WebModelRuntimePanel } from "./webModel/WebModelRuntimePanel";
import { AlertIcon, StopIcon, RefreshIcon, InboxIcon, TrashIcon, PlayIcon, EditIcon, TerminalIcon, PlusIcon } from "./Icons";
import { ScrollFade } from "./ScrollFade";
import { getRuntimeIndicatorState } from "../utils/statusIndicators";
import { getEffectiveActorRunner } from "../utils/headlessRuntimeSupport";
import { copyTextToClipboard } from "../utils/copy";
import { getStoppedTerminalOutputText } from "../utils/stoppedTerminalOutput";
import { fetchTerminalTail } from "../services/api/diagnostics";
import { useAgentTerminalConnection } from "./agentTerminal/useAgentTerminalConnection";

const EMPTY_STREAMING_ACTIVITIES: StreamingActivity[] = [];
const EMPTY_HEADLESS_PREVIEW_SESSIONS: HeadlessPreviewSession[] = [];
const EMPTY_HEADLESS_RAW_EVENTS: HeadlessStreamEvent[] = [];
const STOPPED_TAIL_FETCH_DELAY_MS = 350;

const copyToClipboard = copyTextToClipboard;

export function actorHasRuntimeResumeFailure(actor: Pick<Actor, "runtime_session_status">): boolean {
  return String(actor.runtime_session_status || "").trim().toLowerCase() === "resume_failed";
}

export function shouldFetchStoppedTerminalTail(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  isActorBusy: boolean;
}): boolean {
  return Boolean(args.activated && !args.isRunning && !args.isHeadless && args.groupId && args.actorId && !args.isActorBusy);
}

interface AgentTabProps {
  actor: Actor;
  groupId: string;
  termEpoch?: number;
  agentState: AgentState | null;
  isVisible: boolean;
  readOnly?: boolean;
  onQuit: () => void;
  onLaunch: () => void;
  onRelaunch: () => void;
  onNewSession: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onInbox: () => void;
  busy: string;
  isDark: boolean;
  isSmallScreen: boolean;
  /** Called when the component detects actor status may have changed (e.g., process exited) */
  onStatusChange?: () => void;
}

export function AgentTab({
  actor,
  groupId,
  termEpoch = 0,
  agentState,
  isVisible,
  readOnly,
  onQuit,
  onLaunch,
  onRelaunch,
  onNewSession,
  onEdit,
  onRemove,
  onInbox,
  busy,
  isDark,
  isSmallScreen,
  onStatusChange,
}: AgentTabProps) {
  const { t } = useTranslation('actors');
  // Derived state (must be defined before refs that use them)
  const { isRunning, workingState } = useActorDisplayState({ groupId, actor });
  const effectiveRunner = getEffectiveActorRunner(actor);
  const isHeadless = effectiveRunner === "headless";
  const isWebModel = String(actor.runtime || "").trim().toLowerCase() === "web_model";
  const canStartNewSession = ["claude", "codex"].includes(String(actor.runtime || "").trim().toLowerCase());
  const hasRuntimeResumeFailure = actorHasRuntimeResumeFailure(actor);
  const runtimeResumeError = String(actor.runtime_session_last_resume_error || "").trim();
  const canControl = !readOnly;
  const isBusy = busy.includes(actor.id);
  const latestHeadlessText = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return "";
    const actorId = String(actor.id || "").trim();
    if (!actorId) return "";
    return String(bucket.latestActorTextByActorId?.[actorId] || "");
  });
  const headlessPreviewSessions = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_HEADLESS_PREVIEW_SESSIONS;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_HEADLESS_PREVIEW_SESSIONS;
    const sessions = bucket.previewSessionsByActorId?.[actorId];
    return Array.isArray(sessions) ? sessions : EMPTY_HEADLESS_PREVIEW_SESSIONS;
  });
  const latestHeadlessActivities = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_STREAMING_ACTIVITIES;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_STREAMING_ACTIVITIES;
    const activities = bucket.latestActorActivitiesByActorId?.[actorId];
    return Array.isArray(activities) ? activities : EMPTY_STREAMING_ACTIVITIES;
  });
  const rawHeadlessEvents = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_HEADLESS_RAW_EVENTS;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_HEADLESS_RAW_EVENTS;
    const events = bucket.rawHeadlessEventsByActorId?.[actorId];
    return Array.isArray(events) ? events : EMPTY_HEADLESS_RAW_EVENTS;
  });
  const observabilityLoaded = useObservabilityStore((s) => s.loaded);
  const loadObservability = useObservabilityStore((s) => s.load);
  const terminalScrollbackLines = useObservabilityStore((s) => s.terminalScrollbackLines);
  const setTerminalSignal = useTerminalSignalsStore((s) => s.setSignal);
  const clearTerminalSignal = useTerminalSignalsStore((s) => s.clearSignal);

  const termRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [activated, setActivated] = useState(false);
  // Bumped to trigger a fresh WebSocket connection from the reconnect button
  const [reconnectTrigger, setReconnectTrigger] = useState(0);
  const [stoppedTerminalText, setStoppedTerminalText] = useState("");
  const [stoppedTerminalLoading, setStoppedTerminalLoading] = useState(false);

  const pasteStateRef = useRef<{ inFlight: boolean; lastAt: number }>({ inFlight: false, lastAt: 0 });

  // Ref to avoid stale closure in WebSocket callbacks
  const canControlRef = useRef(canControl);

  // Keep ref in sync with prop
  useEffect(() => {
    canControlRef.current = canControl;
  }, [canControl]);

  // Activate the terminal only after the user has visited this actor tab at least once.
  // Once activated, keep the PTY session connected even when the tab is hidden to avoid backlog replay and scroll jumps.
  useEffect(() => {
    if (isVisible) setActivated(true);
  }, [isVisible]);

  useEffect(() => {
    let cancelled = false;
    setStoppedTerminalText("");
    if (
      !shouldFetchStoppedTerminalTail({
        activated,
        isRunning,
        isHeadless,
        groupId,
        actorId: actor.id,
        isActorBusy: isBusy,
      })
    ) {
      setStoppedTerminalLoading(false);
      return () => {
        cancelled = true;
      };
    }

    setStoppedTerminalLoading(true);
    const timer = window.setTimeout(() => {
      fetchTerminalTail(groupId, actor.id, 8000, true, true)
        .then((resp) => {
          if (cancelled) return;
          setStoppedTerminalText(resp.ok ? getStoppedTerminalOutputText(resp.result.text || "", workingState) : "");
        })
        .catch(() => {
          if (!cancelled) setStoppedTerminalText("");
        })
        .finally(() => {
          if (!cancelled) setStoppedTerminalLoading(false);
        });
    }, STOPPED_TAIL_FETCH_DELAY_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [activated, actor.id, groupId, isBusy, isHeadless, isRunning, termEpoch, workingState]);

  useEffect(() => {
    if (!activated || observabilityLoaded) return;
    void loadObservability();
  }, [activated, loadObservability, observabilityLoaded]);

  const rtInfo = (actor.runtime && RUNTIME_INFO[actor.runtime]) ? RUNTIME_INFO[actor.runtime] : RUNTIME_INFO.codex;
  const unreadCount = actor.unread_count ?? 0;
  const statusClamp2Style: CSSProperties = {
    display: "-webkit-box",
    WebkitBoxOrient: "vertical",
    WebkitLineClamp: 2,
    overflow: "hidden",
  };

  const runtimeIndicator = getRuntimeIndicatorState({ isRunning: Boolean(isRunning), workingState });
  const statusTone = (() => {
    switch (runtimeIndicator.tone) {
      case "stop":
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-slate-500/10 text-slate-500 dark:text-slate-300",
        };
      case "working":
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
        };
      case "run":
      default:
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
        };
    }
  })();

  const runtimeStatusText = (() => {
    if (!isRunning) return t("stopped");
    if (workingState === "working") return t("working");
    return t("running");
  })();
  const handleNewSession = () => {
    if (!window.confirm(t('newSessionConfirm'))) return;
    onNewSession();
  };
  const stoppedTerminalOutputText = getStoppedTerminalOutputText(stoppedTerminalText, workingState);
  const resumeFailureNotice = hasRuntimeResumeFailure ? (
    <div
      className={classNames(
        "flex w-full max-w-xl flex-col items-center rounded-lg border px-4 py-4 text-center",
        "border-amber-500/30 bg-amber-500/10 text-amber-700",
        "dark:border-amber-300/25 dark:bg-amber-300/10 dark:text-amber-100"
      )}
    >
      <AlertIcon size={40} />
      <div className="mt-3 text-lg font-semibold text-[var(--color-text-primary)]">
        {t('runtimeResumeFailedTitle')}
      </div>
      <div className="mt-2 max-w-md text-sm leading-relaxed text-[var(--color-text-secondary)]">
        {t('runtimeResumeFailedDescription')}
      </div>
      {runtimeResumeError ? (
        <pre
          className={classNames(
            "mt-3 max-h-28 w-full overflow-auto whitespace-pre-wrap break-words rounded-md border px-3 py-2 text-left font-mono text-xs leading-relaxed",
            "border-amber-500/25 bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"
          )}
        >
          {runtimeResumeError}
        </pre>
      ) : null}
    </div>
  ) : null;
  const primaryActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-[rgb(35,36,37)] bg-[rgb(35,36,37)] px-3.5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-black disabled:opacity-50 disabled:cursor-not-allowed dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92";
  const secondaryActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3.5 py-2.5 text-sm font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed";
  const ghostActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-transparent px-3 py-2.5 text-sm font-medium text-[var(--color-text-tertiary)] transition-colors hover:border-[var(--glass-border-subtle)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-50 disabled:cursor-not-allowed";

  // Update terminal theme when isDark changes
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.theme = getTerminalTheme(isDark);
    }
  }, [isDark]);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.disableStdin = !canControl;
      terminalRef.current.options.cursorBlink = canControl;
    }
  }, [canControl]);

  // Update terminal scrollback when global settings change.
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.scrollback = terminalScrollbackLines;
    }
  }, [terminalScrollbackLines]);

  // Initialize terminal
  useEffect(() => {
    if (!termRef.current || isHeadless || !isRunning || !activated) return;

    const term = new Terminal({
      cursorBlink: canControl,
      // Avoid an extra blinking "outline" cursor when the terminal isn't focused.
      // Some runtimes render their own cursor; xterm's inactive cursor can look like a second cursor.
      cursorInactiveStyle: "none",
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", Menlo, Monaco, monospace',
      theme: getTerminalTheme(isDark),
      disableStdin: !canControl,
      // Bigger scrollback improves history browsing without going "infinite" and hurting perf.
      // Default is 8k lines; the user can override it in Global → Developer settings.
      scrollback: terminalScrollbackLines || 8000,
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();

    term.loadAddon(fitAddon);
    term.open(termRef.current);
    // Ensure focus works consistently across browsers (and prevents the inactive cursor style).
    const onPointerDown = () => term.focus();
    term.element?.addEventListener("mousedown", onPointerDown);
    term.element?.addEventListener("touchstart", onPointerDown, { passive: true });

    const copySelection = async (): Promise<boolean> => {
      try {
        const sel = term.getSelection ? term.getSelection() : "";
        if (!sel) return false;
        return await copyToClipboard(sel);
      } catch {
        return false;
      }
    };

    // High-ROI copy UX:
    // - If text is selected, Ctrl/Cmd+C copies (instead of sending SIGINT to the runtime)
    // - Right-click copies selection (common web terminal behavior)
    term.attachCustomKeyEventHandler((ev) => {
      const key = (ev.key || "").toLowerCase();
      const isCopy = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && key === "c";
      const isCopyShift = (ev.ctrlKey || ev.metaKey) && ev.shiftKey && key === "c";
      const isPaste = (ev.ctrlKey || ev.metaKey) && !ev.altKey && key === "v";
      if (isCopy || isCopyShift) {
        if (term.hasSelection?.()) {
          void copySelection();
          return false; // prevent ^C from reaching the runtime
        }
      }
      if (isPaste && canControlRef.current) {
        // xterm.js intentionally doesn't map Ctrl+V to paste by default (to preserve terminal semantics),
        // but for CCCC agents the high-ROI expectation is "Ctrl/Cmd+V pastes text into the PTY".
        const readText = navigator.clipboard?.readText;
        if (typeof readText === "function") {
          // Prevent the browser's default paste behavior (xterm's textarea may also handle paste),
          // otherwise we can end up pasting the same payload multiple times.
          ev.preventDefault();
          ev.stopPropagation();

          const now = Date.now();
          if (pasteStateRef.current.inFlight) return false;
          if (now - pasteStateRef.current.lastAt < 250) return false;
          pasteStateRef.current.inFlight = true;
          pasteStateRef.current.lastAt = now;

          void readText.call(navigator.clipboard).then((text: string) => {
            const t = (text || "").toString();
            if (!t) return;
            try {
              term.paste(t);
            } catch {
              // ignore
            }
          }).catch(() => {
            // If clipboard read is blocked, fall back to default behavior.
          }).finally(() => {
            pasteStateRef.current.inFlight = false;
          });
          return false;
        }
      }
      return true;
    });

    const onContextMenu = (ev: MouseEvent) => {
      if (!term.hasSelection?.()) return;
      ev.preventDefault();
      void copySelection();
    };
    term.element?.addEventListener("contextmenu", onContextMenu);

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    // Initial fit — use requestAnimationFrame to wait for layout completion
    requestAnimationFrame(() => {
      if (termRef.current && termRef.current.clientWidth > 50) {
        fitAddon.fit();
      }
    });

    return () => {
      term.element?.removeEventListener("contextmenu", onContextMenu);
      term.element?.removeEventListener("mousedown", onPointerDown);
      term.element?.removeEventListener("touchstart", onPointerDown);
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Theme changes are handled in a dedicated effect; avoid re-creating the terminal.
  }, [isHeadless, isRunning, activated]);

  const {
    connectionStatus,
    terminalReady,
    terminalWritable,
    requestReconnect,
    sendInterrupt,
  } = useAgentTerminalConnection({
    activated,
    isRunning,
    isHeadless,
    groupId,
    actorId: actor.id,
    actorRuntime: actor.runtime,
    canControl,
    termEpoch,
    reconnectTrigger,
    terminalRef,
    onStatusChange,
    setTerminalSignal,
    clearTerminalSignal,
    setReconnectTrigger,
  });

  // Fit terminal on visibility change and resize (with debounce to reduce jitter)
  useEffect(() => {
    if (!isVisible || !fitAddonRef.current) return;

    let resizeTimeout: ReturnType<typeof setTimeout> | null = null;

    const fit = () => {
      if (fitAddonRef.current && termRef.current && termRef.current.clientWidth > 0) {
        fitAddonRef.current.fit();
      }
    };

    // Debounced fit to prevent jitter during rapid resize events
    const debouncedFit = () => {
      if (resizeTimeout) clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(fit, 100);
    };

    // Fit when becoming visible
    setTimeout(fit, 50);

    // Fit on window resize (debounced)
    window.addEventListener("resize", debouncedFit);

    // Observe container resize to catch layout changes (e.g. sidebar toggle, split pane)
    const container = termRef.current;
    let ro: ResizeObserver | null = null;
    if (container) {
      ro = new ResizeObserver(() => debouncedFit());
      ro.observe(container);
    }

    return () => {
      window.removeEventListener("resize", debouncedFit);
      if (ro) ro.disconnect();
      if (resizeTimeout) clearTimeout(resizeTimeout);
    };
  }, [isVisible]);

  // UX: when the user switches to an agent tab (ops mode), focus the terminal automatically.
  // This avoids "typing into nowhere" if the chat composer was previously focused.
  useEffect(() => {
    if (!canControl) return;
    if (!isVisible) return;
    if (!terminalReady || !terminalWritable) return;
    if (isSmallScreen) return;
    const term = terminalRef.current;
    if (!term) return;
    const t = setTimeout(() => {
      try {
        term.focus();
      } catch {
        // ignore
      }
    }, 0);
    return () => clearTimeout(t);
  }, [canControl, isVisible, isSmallScreen, terminalReady, terminalWritable]);

  const stateHeadline = String(agentState?.hot?.focus || agentState?.hot?.next_action || "").trim() || t('noAgentStateYet');
  const stateTask = String(agentState?.hot?.active_task_id || "").trim();
  const blockerCount = Array.isArray(agentState?.hot?.blockers) ? agentState.hot.blockers.length : 0;
  const stateNext = String(agentState?.hot?.next_action || "").trim();

  return (
    <div className="flex flex-col h-full">
      {/* Agent Header */}
      <div className={classNames(
        "border-b px-4 py-2 sm:px-5",
        isDark
          ? "border-white/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.025),rgba(255,255,255,0.01))]"
          : "border-black/6 bg-[linear-gradient(180deg,rgba(255,255,255,0.96),rgba(248,250,252,0.88))]"
      )}>
        <div className="flex items-center justify-between gap-4">
          <div className="flex min-w-0 items-center gap-4">
            <span
              className={classNames(
                "relative inline-flex h-2.5 w-2.5 flex-shrink-0 rounded-full transition-all",
                statusTone.dotClass
              )}
            >
              {statusTone.pulse && (
                <span
                  className={classNames(
                    "absolute inset-[-3px] rounded-full motion-reduce:animate-none",
                    statusTone.strongPulse
                      ? "animate-ping bg-emerald-300/35"
                      : "animate-pulse bg-current/20"
                  )}
                />
              )}
              {statusTone.strongPulse && (
                <span className="absolute inset-[-7px] rounded-full border border-emerald-300/35 animate-ping motion-reduce:animate-none [animation-duration:1.6s]" />
              )}
            </span>

            <div className="flex min-w-0 items-center gap-3">
              <div className="min-w-0 shrink-0">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="min-w-0 truncate font-semibold text-[var(--color-text-primary)]">{actor.title || actor.id}</span>
                  {actor.role === "foreman" && (
                    <span className="rounded-md border border-amber-500/25 bg-amber-500/12 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                      {t('foreman')}
                    </span>
                  )}
                </div>
                <div className={classNames("mt-0.5 text-xs truncate", "text-[var(--color-text-tertiary)]")}>
                  {rtInfo?.label || t('custom')} • {runtimeStatusText}
                  {isHeadless && ` • ${t('headless')}`}
                </div>
                {/* Mobile-only: condensed single-line agent state */}
                <div
                  className={classNames(
                    "sm:hidden mt-1 text-[11px] truncate leading-tight",
                    stateHeadline !== t('noAgentStateYet')
                      ? "text-[var(--color-text-secondary)]"
                      : "text-[var(--color-text-muted)] italic"
                  )}
                  title={stateHeadline}
                >
                  {stateHeadline}
                </div>
              </div>

              <div
                className={classNames(
                  "hidden sm:grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_auto] items-start gap-2 rounded-xl border px-3 py-1.5 backdrop-blur-sm",
                  isDark
                    ? "max-w-[min(660px,54vw)] border-white/10 bg-white/[0.035]"
                    : "max-w-[min(660px,54vw)] border-black/8 bg-white/78"
                )}
                aria-label={t('agentState')}
              >
                <div className="min-w-0">
                  <div
                    className={classNames(
                      "min-w-0 text-sm font-medium leading-[1.15rem]",
                      stateHeadline !== t('noAgentStateYet')
                        ? "text-[var(--color-text-primary)]"
                        : isDark
                          ? "text-slate-500 italic"
                          : "text-gray-500 italic"
                    )}
                    style={statusClamp2Style}
                    title={
                      agentState?.updated_at
                        ? `${stateHeadline}\nUpdated: ${formatFullTime(agentState.updated_at)}`
                        : stateHeadline
                    }
                  >
                    <span>{stateHeadline}</span>
                  </div>
                  {(stateTask || blockerCount > 0 || stateNext) ? (
                    <div className="mt-0.5 flex min-w-0 items-center gap-1.5">
                      {stateTask ? (
                        <span className={classNames("shrink-0 rounded-full bg-[var(--glass-tab-bg)] px-2 py-0.5 text-[10px] text-[var(--color-text-secondary)]")}>
                          {t("taskShort", { id: stateTask })}
                        </span>
                      ) : null}
                      {blockerCount > 0 ? (
                        <span className={classNames("shrink-0 rounded-full bg-rose-500/15 px-2 py-0.5 text-[10px] text-rose-600 dark:text-rose-300")}>
                          {t("blockersShort", { count: blockerCount })}
                        </span>
                      ) : null}
                      {stateNext ? (
                        <span
                          className={classNames("min-w-0 truncate text-[10px] leading-4", "text-[var(--color-text-tertiary)]")}
                          title={stateNext}
                        >
                          {t("nextShort", { value: stateNext })}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                </div>
                {agentState?.updated_at ? (
                  <div className="shrink-0 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-2 py-0.5 text-[10px] font-medium leading-4 text-[var(--color-text-tertiary)]">
                    {formatTime(agentState.updated_at)}
                  </div>
                ) : null}
              </div>
            </div>
          </div>

        </div>
      </div>

      {/* Terminal or Status Area */}
      {/* contain: layout prevents terminal content changes from triggering parent layout recalculation */}
      <div className={classNames("flex-1 min-h-0 relative", "bg-[var(--color-bg-secondary)]")} style={{ contain: 'layout', overflow: 'hidden' }}>
        {isHeadless ? (
          <div className="flex h-full min-h-0 flex-col px-5 pb-5 pt-3 sm:px-7 sm:pb-6 sm:pt-3">
            <div
              className={classNames(
                "mx-auto flex w-full min-h-0 flex-1 flex-col",
                isWebModel ? "max-w-none gap-3" : "max-w-6xl gap-4",
              )}
            >
              {isWebModel ? (
                <WebModelRuntimePanel
                  groupId={groupId}
                  actor={actor}
                  isDark={isDark}
                  isVisible={isVisible}
                  isRunning={isRunning}
                  readOnly={readOnly}
                />
              ) : null}
              {!isWebModel ? (
                <div className="min-h-0 flex-1">
                  {resumeFailureNotice && !isRunning ? (
                    <div className="flex h-full min-h-[420px] items-center justify-center">
                      {resumeFailureNotice}
                    </div>
                  ) : (
                    <HeadlessRuntimePanel
                      actorId={actor.id}
                      previewSessions={headlessPreviewSessions}
                      fallbackText={latestHeadlessText}
                      fallbackActivities={latestHeadlessActivities}
                      rawEvents={rawHeadlessEvents}
                      emptyLabel={t('noStreamingOutputYet', { defaultValue: 'There is no streaming output to show yet.' })}
                      isDark={isDark}
                    />
                  )}
                </div>
              ) : null}
            </div>
          </div>
        ) : isRunning ? (
          // PTY agent - show terminal
          // contain: layout paint isolates layout/paint calculations to prevent jitter when terminal content updates
          // opacity transition hides initial backlog replay scrolling
          <>
            <div
              ref={termRef}
              className="h-full w-full transition-opacity duration-100"
              style={{
                contain: 'layout paint',
                overflow: 'hidden',
                opacity: terminalReady ? 1 : 0,
              }}
            />
            {/* Connection error overlay — shown when all reconnect attempts failed and terminal never became ready */}
            {connectionStatus === 'disconnected' && !terminalReady && (
              <div className={classNames(
                "absolute inset-0 flex flex-col items-center justify-center p-8",
                "text-[var(--color-text-tertiary)] bg-[var(--glass-panel-bg)]"
              )}>
                <div className="mb-4"><TerminalIcon size={48} /></div>
                <div className="text-lg font-medium mb-2">{t('connectionLost')}</div>
                <div className="text-sm text-center max-w-md mb-4">
                  {t('connectionLostDescription')}
                </div>
                {canControl && (
                  <button
                    onClick={requestReconnect}
                    className="flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium min-h-[44px] transition-colors"
                  >
                    <RefreshIcon size={16} />
                    {t('reconnect')}
                  </button>
                )}
              </div>
            )}
            {canControl && connectionStatus === 'connected' && terminalReady && !terminalWritable ? (
              <div className="absolute right-4 top-4 z-10 rounded-lg border border-amber-500/30 bg-amber-500/12 px-3 py-2 text-xs font-medium text-amber-700 shadow-sm backdrop-blur dark:text-amber-200">
                {t('terminalReadOnlyNotice', { defaultValue: 'Terminal is connected read-only. Reconnect to take control.' })}
              </div>
            ) : null}
          </>
        ) : (
          // Stopped agent
          <div className={classNames("flex flex-col items-center h-full p-8 overflow-y-auto", "text-[var(--color-text-tertiary)]")}>
            <div className="flex flex-col items-center flex-shrink-0">
              {resumeFailureNotice ? (
                <div className="mb-4">{resumeFailureNotice}</div>
              ) : (
                <>
                  <div className="mb-4"><TerminalIcon size={48} /></div>
                  <div className="text-lg font-medium mb-2">{t('agentNotRunning')}</div>
                  <div className="text-sm text-center max-w-md mb-4">
                    {t('agentStoppedDescription')}
                  </div>
                </>
              )}
              {canControl ? (
                <button
                  onClick={onLaunch}
                  disabled={isBusy}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium disabled:opacity-50 min-h-[44px] transition-colors"
                  aria-label={t('launchAgentLabel')}
                >
                  <PlayIcon size={16} />
                  {isBusy ? t('launching') : t('launchAgent')}
                </button>
              ) : null}
            </div>
            <div className="mt-6 w-full max-w-xl flex-shrink-0 rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-4 py-3 text-sm text-[var(--color-text-secondary)]">
              {stoppedTerminalLoading ? (
                t('loadingLastTerminalOutput')
              ) : stoppedTerminalOutputText ? (
                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words text-left font-mono text-xs leading-relaxed">
                  {stoppedTerminalOutputText}
                </pre>
              ) : (
                t('noRecentTerminalOutput')
              )}
            </div>
          </div>
        )}
      </div>

      {/* Action Buttons - Scrollable on mobile with fade edges */}
      {canControl ? (
        <ScrollFade
          className={classNames(
            "border-t select-none",
            "glass-header"
          )}
          innerClassName="flex items-center gap-2 px-4 py-3 sm:px-5"
          fadeWidth={20}
        >
          {isRunning ? (
            <>
              <button
                onClick={onQuit}
                disabled={isBusy}
                className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('quitAgent')}
              >
                <StopIcon size={16} />
                {!isSmallScreen && t('quit')}
              </button>
              <button
                onClick={sendInterrupt}
                disabled={connectionStatus !== 'connected' || !terminalWritable}
                className={`${ghostActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                title={t('sendInterruptTitle')}
                aria-label={t('sendInterruptLabel')}
              >
                ⌃C
              </button>
              <button
                onClick={onRelaunch}
                disabled={isBusy}
                className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('relaunchAgent')}
              >
                <RefreshIcon size={16} />
                {!isSmallScreen && t('relaunch')}
              </button>
              {canStartNewSession ? (
                <button
                  onClick={handleNewSession}
                  disabled={isBusy}
                  className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                  aria-label={t('newSessionAgent')}
                >
                  <PlusIcon size={16} />
                  {!isSmallScreen && t('newSession')}
                </button>
              ) : null}
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={`${ghostActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('editAgentConfig')}
              >
                <EditIcon size={16} />
                {!isSmallScreen && t('common:edit')}
              </button>
            </>
          ) : (
            <>
              <button
                onClick={onLaunch}
                disabled={isBusy}
                className={`${primaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('launchAgentLabel')}
              >
                <PlayIcon size={16} />
                {isBusy ? t('launching') : t('launch')}
              </button>
              {canStartNewSession ? (
                <button
                  onClick={handleNewSession}
                  disabled={isBusy}
                  className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                  aria-label={t('newSessionAgent')}
                >
                  <PlusIcon size={16} />
                  {t('newSession')}
                </button>
              ) : null}
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={ghostActionButtonClass}
                aria-label={t('editAgentConfig')}
              >
                <EditIcon size={16} /> {t('common:edit')}
              </button>
            </>
          )}
          <button
            onClick={onInbox}
            className={classNames(
              "ml-auto flex items-center gap-1.5 rounded-xl px-3.5 py-2.5 text-sm min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap border",
              unreadCount > 0
                ? isDark
                  ? "border-white/12 bg-white/[0.08] text-white hover:bg-white/[0.12]"
                  : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white"
                : isDark
                  ? "border-white/10 bg-white/[0.06] hover:bg-white/[0.1] text-white"
                  : "border-black/10 bg-white hover:bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
            )}
            aria-label={`${t('openInbox')}${unreadCount > 0 ? t('unreadMessages', { count: unreadCount }) : ""}`}
          >
            <InboxIcon size={16} />
            {!isSmallScreen && t('inbox')}
            {unreadCount > 0 && (
              <span
                className={classNames(
                  "text-[10px] px-1.5 py-0.5 rounded-full font-semibold tracking-tight shadow-sm",
                  isDark ? "bg-white text-[rgb(20,20,22)]" : "bg-[rgb(35,36,37)] text-white"
                )}
                aria-hidden="true"
              >
                {unreadCount > 99 ? "99+" : unreadCount}
              </span>
            )}
          </button>
          <button
            onClick={onRemove}
            disabled={isBusy || isRunning}
            className={classNames(
              "flex items-center gap-1.5 rounded-xl px-3 py-2.5 text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
              "text-rose-600 hover:bg-rose-500/10 dark:text-rose-400"
            )}
            title={isRunning ? t('stopBeforeRemoving') : t('removeAgent')}
            aria-label={t('removeAgent')}
          >
            <TrashIcon size={16} />
            {!isSmallScreen && t('common:remove')}
          </button>
        </ScrollFade>
      ) : null}
    </div>
  );
}
