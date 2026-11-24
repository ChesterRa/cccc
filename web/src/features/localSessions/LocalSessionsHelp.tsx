import { useState } from "react";
import { useTranslation } from "react-i18next";
import { LocalSessionsSetup } from "./LocalSessionsSetup";

export function LocalSessionsHelp({
  onRefreshSessions,
  defaultSetupOpen = false,
}: {
  onRefreshSessions?: () => void;
  defaultSetupOpen?: boolean;
}) {
  const { t } = useTranslation("layout");
  const [setupOpen, setSetupOpen] = useState(defaultSetupOpen);
  const [troubleshootingOpen, setTroubleshootingOpen] = useState(false);
  return (
    <div className="space-y-2 text-sm text-[var(--color-text-secondary)]">
      <details open={setupOpen} onToggle={(event) => setSetupOpen(event.currentTarget.open)}>
        <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
          {t("localSessions.setup")}
        </summary>
        <LocalSessionsSetup onRefreshSessions={onRefreshSessions} />
      </details>
      <details
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
            <li>{t("localSessions.troubleshootPi")}</li>
            <li>{t("localSessions.troubleshootOpencode")}</li>
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
