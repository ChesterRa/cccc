import { useTranslation } from "react-i18next";
import type { LocalSessionSource } from "../../types";

const runtimeNames: Record<string, string> = {
  codex: "Codex",
  claude: "Claude Code",
  pi: "Pi",
  opencode: "OpenCode",
};

export function LocalSessionOrigin({ source }: { source: LocalSessionSource }) {
  const { t } = useTranslation("layout");
  const remote = source.origin?.startsWith("remote:") ? source.origin.slice(7) : null;
  const origin =
    !remote && source.runtime === "codex"
      ? source.origin === "cli"
        ? "CLI"
        : source.origin === "vscode"
          ? "VS Code"
          : null
      : null;
  return (
    <span className="flex min-w-0 items-center gap-2 text-xs text-[var(--color-text-secondary)]">
      <span className="shrink-0">{runtimeNames[source.runtime] || source.runtime}</span>
      {origin && (
        <>
          <span aria-hidden="true">·</span>
          <span className="shrink-0" title={t("localSessions.originHint", { origin })}>
            {origin}
          </span>
        </>
      )}
      <span aria-hidden="true">·</span>
      <span className="min-w-0 truncate text-[var(--color-text-muted)]" title={source.cwd}>
        {source.cwd}
      </span>
    </span>
  );
}

export function LocalSessionStatus({ connected }: { connected: boolean }) {
  const { t } = useTranslation("layout");
  return (
    <span
      className={`inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-xs ${connected ? "text-[var(--color-text-muted)]" : "text-amber-700 dark:text-amber-300"}`}
      data-status-tone={connected ? undefined : "warning"}
    >
      <span
        className="h-1.5 w-1.5 bg-current"
        data-status-dot={connected ? "active" : "attention"}
        aria-hidden="true"
      />
      {t(connected ? "localSessions.ready" : "localSessions.unavailable")}
    </span>
  );
}
