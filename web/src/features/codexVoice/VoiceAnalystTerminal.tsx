import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshIcon, TerminalIcon } from "../../components/Icons";
import { useAgentTerminalConnection } from "../../components/agentTerminal/useAgentTerminalConnection";
import { attachTerminalTouchScroll } from "../../components/agentTerminal/terminalTouchScroll";
import { getTerminalTheme } from "../../components/agentTerminal/terminalTheme";
import { Button } from "../../components/ui/button";
import { useObservabilityStore } from "../../stores";
import type { CodexVoiceAnalystInfo } from "../../services/api";
import { getCodexVoiceTerminalWebSocketUrl } from "../../services/api";
import { copyTextToClipboard } from "../../utils/copy";

type Props = { analyst: CodexVoiceAnalystInfo; isVisible: boolean };

export function VoiceAnalystTerminal({ analyst, isVisible }: Props) {
  const { t } = useTranslation("modals");
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [reconnectTrigger, setReconnectTrigger] = useState(0);
  const scrollbackLines = useObservabilityStore((state) => state.terminalScrollbackLines);

  const fit = useCallback(() => {
    const container = containerRef.current;
    const addon = fitAddonRef.current;
    if (!container || !addon || container.clientWidth <= 50 || container.clientHeight <= 50) {
      return;
    }
    try {
      addon.fit();
    } catch {
      // The next ResizeObserver or visibility fit retries after layout settles.
    }
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const terminal = new Terminal({
      cursorBlink: true,
      cursorInactiveStyle: "none",
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", Menlo, Monaco, monospace',
      theme: getTerminalTheme(),
      scrollback: 8000,
      allowProposedApi: true,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(container);
    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    const focus = () => terminal.focus();
    terminal.element?.addEventListener("mousedown", focus);
    const detachTouchScroll = attachTerminalTouchScroll(terminal);
    const copySelection = async () => {
      const selection = terminal.getSelection?.() || "";
      return selection ? copyTextToClipboard(selection) : false;
    };
    terminal.attachCustomKeyEventHandler((event) => {
      const key = String(event.key || "").toLowerCase();
      if ((event.ctrlKey || event.metaKey) && key === "c" && terminal.hasSelection?.()) {
        void copySelection();
        return false;
      }
      if ((event.ctrlKey || event.metaKey) && key === "v") {
        const readText = navigator.clipboard?.readText;
        if (typeof readText === "function") {
          event.preventDefault();
          void readText
            .call(navigator.clipboard)
            .then((text) => {
              if (text) terminal.paste(text);
            })
            .catch(() => undefined);
          return false;
        }
      }
      return true;
    });
    const contextCopy = (event: MouseEvent) => {
      if (!terminal.hasSelection?.()) return;
      event.preventDefault();
      void copySelection();
    };
    terminal.element?.addEventListener("contextmenu", contextCopy);

    const frame = requestAnimationFrame(fit);
    return () => {
      cancelAnimationFrame(frame);
      detachTouchScroll();
      terminal.element?.removeEventListener("mousedown", focus);
      terminal.element?.removeEventListener("contextmenu", contextCopy);
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [analyst.generation, fit]);

  useEffect(() => {
    if (terminalRef.current) terminalRef.current.options.scrollback = scrollbackLines || 8000;
  }, [scrollbackLines]);

  useEffect(() => {
    if (!isVisible) return;
    const timers = [0, 60, 180].map((delay) => window.setTimeout(fit, delay));
    const observer = new ResizeObserver(fit);
    if (containerRef.current) observer.observe(containerRef.current);
    window.addEventListener("resize", fit);
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
      observer.disconnect();
      window.removeEventListener("resize", fit);
    };
  }, [fit, isVisible]);

  const buildCustomWebSocketUrl = useCallback(
    (query: string) => getCodexVoiceTerminalWebSocketUrl(analyst.generation, query),
    [analyst.generation],
  );
  const noopSignal = useCallback(() => undefined, []);
  const {
    connectionStatus,
    connectionFailed,
    terminalReady,
    terminalWritable,
    requestReconnect,
    sendInterrupt,
  } = useAgentTerminalConnection({
    activated: isVisible,
    isRunning: true,
    isHeadless: false,
    groupId: "codex-voice",
    actorId: analyst.generation,
    actorRuntime: "codex",
    canControl: true,
    termEpoch: 0,
    reconnectTrigger,
    terminalRef,
    fitBeforeAttach: fit,
    setTerminalSignal: noopSignal,
    clearTerminalSignal: noopSignal,
    setReconnectTrigger,
    buildCustomWebSocketUrl,
    inspectActorTail: false,
  });

  return (
    <div className="flex h-full min-h-0 flex-col bg-[var(--color-chat-bg)]">
      <div className="relative min-h-0 flex-1 bg-[#111214]">
        <div
          ref={containerRef}
          className="h-full w-full transition-opacity duration-100"
          style={{ contain: "layout paint", overflow: "hidden", opacity: terminalReady ? 1 : 0 }}
        />
        {connectionStatus === "disconnected" && !terminalReady ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center bg-[var(--glass-panel-bg)] p-8 text-center text-[var(--color-text-tertiary)]">
            <TerminalIcon size={42} />
            <div className="mt-4 text-base font-medium text-[var(--color-text-primary)]">
              {t("codexVoiceTerminalDisconnected")}
            </div>
            <p className="mt-2 max-w-md text-sm leading-6">
              {connectionFailed
                ? t("codexVoiceTerminalRejected")
                : t("codexVoiceTerminalReconnectHint")}
            </p>
            <Button type="button" variant="secondary" className="mt-4" onClick={requestReconnect}>
              <RefreshIcon size={15} />
              {t("codexVoiceTerminalReconnect")}
            </Button>
          </div>
        ) : null}
      </div>

      <footer className="flex flex-none items-center justify-between gap-3 border-t border-white/10 bg-[#111214] px-3 py-2">
        <p className="min-w-0 truncate text-xs text-[var(--color-text-muted)]">
          {t("codexVoiceTerminalLifecycleHint")}
        </p>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={sendInterrupt}
          disabled={connectionStatus !== "connected" || !terminalWritable}
          className="flex-none"
        >
          Ctrl+C
        </Button>
      </footer>
    </div>
  );
}
