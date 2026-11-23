import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { LocalSessionsSetup } from "./LocalSessionsSetup";

export type LocalSessionsHelpRequest = { section: "setup" | "troubleshooting"; sequence: number };

export function LocalSessionsHelp({
  request,
  onChooseSessions,
}: {
  request: LocalSessionsHelpRequest | null;
  onChooseSessions: () => void;
}) {
  const { t } = useTranslation("layout");
  const [setupOpen, setSetupOpen] = useState(false);
  const [troubleshootingOpen, setTroubleshootingOpen] = useState(false);
  const setupRef = useRef<HTMLDetailsElement>(null);
  const troubleshootingRef = useRef<HTMLDetailsElement>(null);
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
    <div className="space-y-2 text-sm text-[var(--color-text-secondary)]">
      <details
        ref={setupRef}
        open={setupOpen}
        onToggle={(event) => setSetupOpen(event.currentTarget.open)}
      >
        <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
          {t("localSessions.setup")}
        </summary>
        <LocalSessionsSetup onChooseSessions={onChooseSessions} />
      </details>
      <details
        ref={troubleshootingRef}
        open={troubleshootingOpen}
        onToggle={(event) => setTroubleshootingOpen(event.currentTarget.open)}
      >
        <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
          {t("localSessions.troubleshooting")}
        </summary>
        <div className="space-y-3 pb-3 pt-1 text-sm leading-relaxed">
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
        className="inline-block py-1 text-sm underline underline-offset-2"
        href="https://chesterra.github.io/cccc/guide/local-sessions"
        target="_blank"
        rel="noopener noreferrer"
      >
        {t("localSessions.setupGuide")}
      </a>
    </div>
  );
}
