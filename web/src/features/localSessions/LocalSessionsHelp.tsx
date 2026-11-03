import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SelectMenu } from "../../components/ui/select-menu";

export type LocalSessionsHelpRequest = { section: "setup" | "troubleshooting"; sequence: number };
const runtimes = [
  { value: "codex", label: "Codex", instruction: "setupCodex" },
  { value: "claude", label: "Claude Code", instruction: "setupClaude" },
  { value: "pi", label: "Pi", instruction: "setupPi" },
  { value: "opencode", label: "OpenCode V2", instruction: "setupOpencode" },
];

export function LocalSessionsHelp({
  request,
  openSetup,
}: {
  request: LocalSessionsHelpRequest | null;
  /** The first list came back empty: setup opens by itself, without scrolling to it. */
  openSetup: boolean;
}) {
  const { t } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  const [setupOpen, setSetupOpen] = useState(false);
  const [troubleshootingOpen, setTroubleshootingOpen] = useState(false);
  const setupRef = useRef<HTMLDetailsElement>(null);
  const troubleshootingRef = useRef<HTMLDetailsElement>(null);
  const current = runtimes.find((item) => item.value === runtime) || runtimes[0];
  useEffect(() => {
    if (openSetup) setSetupOpen(true);
  }, [openSetup]);
  useEffect(() => {
    if (request?.section === "setup") setSetupOpen(true);
    if (request?.section === "troubleshooting") setTroubleshootingOpen(true);
  }, [request]);
  // Scroll to a requested section once, when it has opened; opening or closing either
  // section later leaves the view where the reader put it.
  const scrolledFor = useRef(0);
  useEffect(() => {
    if (!request || scrolledFor.current === request.sequence) return;
    const target = request.section === "setup" ? setupRef.current : troubleshootingRef.current;
    if (!target?.open) return;
    scrolledFor.current = request.sequence;
    target.scrollIntoView?.({ block: "nearest" });
  }, [request, setupOpen, troubleshootingOpen]);
  return (
    <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3 text-sm text-[var(--color-text-secondary)]">
      <details
        ref={setupRef}
        open={setupOpen}
        onToggle={(event) => setSetupOpen(event.currentTarget.open)}
      >
        <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
          {t("localSessions.setup")}
        </summary>
        <div className="space-y-4 pb-3 pt-1 text-xs leading-relaxed">
          <p>{t("localSessions.setupHint")}</p>
          <div className="space-y-2 rounded-lg bg-[var(--glass-tab-bg)] p-3">
            <p>{t("localSessions.setupTerminal")}</p>
            <pre className="overflow-x-auto font-mono">/setup</pre>
          </div>
          <div className="space-y-2">
            <p>{t("localSessions.setupHost")}</p>
            <p className="font-medium text-[var(--color-text-primary)]">
              {t("localSessions.setupRuntime")}
            </p>
            <SelectMenu
              ariaLabel={t("localSessions.setupRuntime")}
              value={runtime}
              onChange={setRuntime}
              options={runtimes}
              align="start"
              className="w-full"
              contentClassName="w-[var(--radix-popover-trigger-width)]"
            />
            <pre className="overflow-x-auto rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-3 font-mono">{`cccc sessions enable --runtime ${current.value}`}</pre>
            <p className="font-medium text-[var(--color-text-primary)]">
              {t("localSessions.setupNext")}
            </p>
            <p>{t(`localSessions.${current.instruction}`)}</p>
          </div>
          <details>
            <summary className="cursor-pointer py-1 font-medium text-[var(--color-text-primary)]">
              {t("localSessions.setupExampleHeading")}
            </summary>
            <div className="space-y-2 pt-2">
              <p>{t("localSessions.setupStart")}</p>
              <blockquote className="break-words border-l-2 border-[var(--color-border-secondary)] pl-3">
                {t("localSessions.setupExample")}
              </blockquote>
              <p>{t("localSessions.setupConversationHint")}</p>
            </div>
          </details>
        </div>
      </details>
      <details
        ref={troubleshootingRef}
        open={troubleshootingOpen}
        onToggle={(event) => setTroubleshootingOpen(event.currentTarget.open)}
      >
        <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
          {t("localSessions.troubleshooting")}
        </summary>
        <div className="space-y-3 pb-3 pt-1 text-xs leading-relaxed">
          <p>{t("localSessions.setupCheck")}</p>
          <pre className="overflow-x-auto rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-3 font-mono">
            cccc sessions list
          </pre>
          <ul className="list-disc space-y-2 pl-5">
            <li>{t("localSessions.setupReload")}</li>
            <li>{t("localSessions.troubleshootCodex")}</li>
            <li>{t("localSessions.troubleshootClaude")}</li>
          </ul>
        </div>
      </details>
      <a
        className="inline-block py-1 text-xs underline underline-offset-2"
        href="https://chesterra.github.io/cccc/guide/local-sessions"
        target="_blank"
        rel="noopener noreferrer"
      >
        {t("localSessions.setupGuide")}
      </a>
    </div>
  );
}
