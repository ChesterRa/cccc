import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { classNames } from "../../utils/classNames";
import { Cable, Globe2 } from "lucide-react";
import { LcdIcon } from "../../components/Icons";
import { DirectConnectionsPanel } from "./DirectConnectionsPanel";
import { AccountGroupConnectionsPanel } from "./AccountGroupConnectionsPanel";

export function GroupConnectionsPanel(props: {
  groupId: string;
  groupTitle?: string;
  invitation?: string;
  fill?: boolean;
  onOpenAccount: () => void;
}) {
  const { t } = useTranslation("layout");
  const modeId = useId();
  const [mode, setMode] = useState<"account" | "direct">("account");
  const active = props.invitation ? "account" : mode;
  return (
    <div className={props.fill ? "flex h-full min-h-0 flex-col gap-3" : "space-y-4"}>
      {!props.invitation && (
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3">
          <div
            className="grid w-full grid-cols-2 gap-1 rounded-lg bg-[var(--glass-tab-bg)] p-1 sm:inline-flex sm:w-auto"
            role="group"
            aria-label={t("groupConnections.title")}
            data-connect-mode-switch
          >
            {(["account", "direct"] as const).map((option) => (
              <Button
                variant="ghost"
                key={option}
                type="button"
                aria-pressed={active === option}
                aria-label={t(option === "account" ? "direct.account" : "direct.title")}
                aria-describedby={`${modeId}-${option}`}
                className={classNames(
                  "h-auto min-h-11 min-w-0 justify-center gap-2 rounded-md px-3 py-2 text-sm whitespace-normal duration-200 motion-reduce:!transition-none",
                  active === option
                    ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-sm"
                    : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]",
                )}
                onClick={() => setMode(option)}
              >
                {option === "account" ? (
                  <LcdIcon
                    icon={Globe2}
                    glyph="globe"
                    aria-hidden="true"
                    className="h-4 w-4 shrink-0"
                  />
                ) : (
                  <LcdIcon
                    icon={Cable}
                    glyph="link"
                    aria-hidden="true"
                    className="h-4 w-4 shrink-0"
                  />
                )}
                <span className="min-w-0">
                  <span className="block">
                    {t(option === "account" ? "direct.account" : "direct.title")}
                  </span>
                  <span id={`${modeId}-${option}`} className="sr-only">
                    {t(
                      option === "account"
                        ? "groupConnections.accountRequirement"
                        : "groupConnections.directRequirement",
                    )}
                  </span>
                </span>
              </Button>
            ))}
          </div>
          <p className="text-sm text-[var(--color-text-secondary)]" aria-hidden="true">
            {t(
              active === "account"
                ? "groupConnections.accountRequirement"
                : "groupConnections.directRequirement",
            )}
          </p>
        </div>
      )}
      <div
        key={active}
        role="region"
        aria-label={t(active === "account" ? "direct.account" : "direct.title")}
        className={classNames(
          "motion-safe:animate-fade-in motion-reduce:animate-none",
          props.fill ? "scrollbar-hide min-h-0 flex-1 overflow-y-auto" : "",
        )}
      >
        {active === "direct" ? (
          <DirectConnectionsPanel
            fill={props.fill}
            key={props.groupId}
            groupId={props.groupId}
            groupTitle={props.groupTitle}
          />
        ) : (
          <AccountGroupConnectionsPanel {...props} />
        )}
      </div>
    </div>
  );
}
