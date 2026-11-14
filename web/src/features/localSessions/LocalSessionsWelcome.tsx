import { Button } from "../../components/ui/button";
import { ChevronRight, Laptop, Monitor, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ActorAvatar } from "../../components/ActorAvatar";

const supportedRuntimes = [
  { runtime: "codex", name: "Codex" },
  { runtime: "claude", name: "Claude Code" },
  { runtime: "pi", name: "Pi" },
  { runtime: "opencode", name: "OpenCode" },
];

function EntryRow({
  icon: Icon,
  title,
  description,
  descriptionId,
  onClick,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  descriptionId: string;
  onClick: () => void;
}) {
  return (
    <li>
      <Button
        variant="ghost"
        type="button"
        aria-label={title}
        aria-describedby={descriptionId}
        onClick={onClick}
        className="group flex h-auto min-h-16 w-full justify-start rounded-none whitespace-normal items-center gap-4 px-4 py-3 text-left transition-colors hover:bg-[var(--glass-tab-bg-hover)]"
      >
        <span className="flex h-10 w-10 shrink-0 items-center justify-center border border-[var(--glass-border)] text-[var(--color-text-secondary)] group-hover:text-[var(--color-text-primary)]">
          <Icon size={20} strokeWidth={1.5} aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-semibold">{title}</span>
          <span id={descriptionId} className="block text-sm text-[var(--color-text-secondary)]">
            {description}
          </span>
        </span>
        <ChevronRight
          size={18}
          strokeWidth={1.5}
          className="shrink-0 text-[var(--color-text-muted)] transition-transform group-hover:translate-x-0.5 group-hover:text-[var(--color-text-primary)] motion-reduce:transition-none"
          aria-hidden="true"
        />
      </Button>
    </li>
  );
}

export function LocalSessionsWelcome({
  isDark,
  onLocalSetup,
  onAddDevice,
}: {
  isDark: boolean;
  onLocalSetup: () => void;
  onAddDevice: () => void;
}) {
  const { t } = useTranslation("layout");
  return (
    <section className="mx-auto w-full max-w-xl space-y-5" aria-labelledby="local-sessions-empty">
      <div className="space-y-3">
        <p
          role="status"
          className="flex items-center gap-2 text-xs uppercase tracking-wider text-[var(--color-text-muted)]"
        >
          <span
            className="h-2 w-2 animate-pulse bg-current motion-reduce:animate-none"
            aria-hidden="true"
          />
          {t("localSessions.listening")}
        </p>
        <h3 id="local-sessions-empty" className="text-lg font-semibold">
          {t("localSessions.empty")}
        </h3>
        <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
          {t("localSessions.emptyDescription")}
        </p>
        <ul className="flex flex-wrap gap-2" aria-label={t("localSessions.setupRuntime")}>
          {supportedRuntimes.map(({ runtime, name }) => (
            <li
              key={runtime}
              className="flex items-center gap-2 border border-[var(--glass-border)] py-1 pl-1 pr-2.5 text-xs"
            >
              <ActorAvatar
                runtime={runtime}
                title={name}
                isDark={isDark}
                sizeClassName="h-5 w-5"
                textClassName="text-[10px]"
              />
              {name}
            </li>
          ))}
        </ul>
      </div>
      <ul className="divide-y divide-[var(--glass-border)] border border-[var(--glass-border)]">
        <EntryRow
          icon={Monitor}
          title={t("localSessions.localSetup")}
          description={t("localSessions.localSetupDescription")}
          descriptionId="local-session-setup-description"
          onClick={onLocalSetup}
        />
        <EntryRow
          icon={Laptop}
          title={t("localSessions.addDevice")}
          description={t("localSessions.deviceSetupDescription")}
          descriptionId="remote-session-setup-description"
          onClick={onAddDevice}
        />
      </ul>
    </section>
  );
}
