import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { SessionCommand } from "./SessionCommand";

const runtimes = [
  { value: "codex", label: "Codex", instruction: "setupCodex" },
  { value: "claude", label: "Claude Code", instruction: "setupClaude" },
  { value: "pi", label: "Pi", instruction: "setupPi" },
  { value: "opencode", label: "OpenCode V2", instruction: "setupOpencode" },
];

export function LocalSessionsSetup({ onChooseSessions }: { onChooseSessions: () => void }) {
  const { t } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  const current = runtimes.find((item) => item.value === runtime) || runtimes[0];
  return (
    <div className="space-y-4 pb-3 pt-2 text-sm leading-relaxed">
      <ol className="list-decimal space-y-5 pl-5 marker:font-semibold marker:text-[var(--color-text-primary)]">
        <li className="min-w-0 space-y-2 pl-1">
          <h4 className="font-medium text-[var(--color-text-primary)]">
            {t("localSessions.setupEnableHeading")}
          </h4>
          <p>{t("localSessions.setupHost")}</p>
          <SelectMenu
            ariaLabel={t("localSessions.setupRuntime")}
            value={runtime}
            onChange={setRuntime}
            options={runtimes}
            align="start"
            className="min-h-11 w-full text-sm"
            contentClassName="w-[var(--radix-popover-trigger-width)] [&_[role=menuitemradio]]:text-sm"
          />
          <SessionCommand
            key={runtime}
            command={`cccc sessions enable --runtime ${current.value}`}
          />
        </li>
        <li className="space-y-2 pl-1">
          <h4 className="font-medium text-[var(--color-text-primary)]">
            {t("localSessions.setupNext", { app: current.label })}
          </h4>
          <p>{t(`localSessions.${current.instruction}`)}</p>
        </li>
        <li className="space-y-2 pl-1">
          <h4 className="font-medium text-[var(--color-text-primary)]">
            {t("localSessions.setupFinishHeading")}
          </h4>
          <Button
            type="button"
            variant="outline"
            className="min-h-11 text-sm"
            onClick={onChooseSessions}
          >
            {t("localSessions.showSessions")}
          </Button>
        </li>
      </ol>
      <div className="space-y-1 border-t border-[var(--glass-border-subtle)] pt-2">
        <details>
          <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
            {t("localSessions.remoteHeading")}
          </summary>
          <div className="space-y-2 pb-2">
            <p>{t("localSessions.remoteBrowser")}</p>
            <p>{t("localSessions.remoteHost")}</p>
          </div>
        </details>
        <details>
          <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
            {t("localSessions.setupTerminalHeading")}
          </summary>
          <p className="pb-2">{t("localSessions.setupTerminal")}</p>
          <pre className="overflow-x-auto rounded-lg bg-[var(--glass-tab-bg)] p-3 font-mono text-sm">
            /setup
          </pre>
        </details>
        <details>
          <summary className="cursor-pointer py-2 font-medium text-[var(--color-text-primary)]">
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
    </div>
  );
}
