import { useTranslation } from "react-i18next";

import { CloseIcon } from "../../components/Icons";
import { ScrollFade } from "../../components/ScrollFade";
import type { Actor } from "../../types";
import { classNames } from "../../utils/classNames";
import { RecipientPopover } from "./RecipientPopover";
import { useRecipientPopover } from "./useRecipientPopover";

interface ComposerRecipientsRowProps {
  isDark: boolean;
  isSmallScreen: boolean;
  selectedGroupId: string;
  busy: string;
  actors: Actor[];
  selectedGroupActorsHydrating?: boolean;
  toTokens: string[];
  onToggleRecipient: (token: string) => void;
  onClearRecipients: () => void;
}

export function ComposerRecipientsRow({
  isDark,
  isSmallScreen,
  selectedGroupId,
  busy,
  actors,
  selectedGroupActorsHydrating,
  toTokens,
  onToggleRecipient,
  onClearRecipients,
}: ComposerRecipientsRowProps) {
  const { t } = useTranslation("chat");
  const popover = useRecipientPopover({ isSmallScreen });
  const actorChipDisabled = !selectedGroupId || busy === "send" || !!selectedGroupActorsHydrating;
  const chipBaseClass =
    "flex h-6 flex-shrink-0 items-center justify-center whitespace-nowrap rounded-lg border px-2 text-xs font-medium leading-none transition-all sm:px-2.5 sm:text-xs";
  const chipActiveClass = isDark
    ? "border-white bg-white text-[rgb(20,20,22)] shadow-none"
    : "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white shadow-none";
  const chipInactiveClass = isDark
    ? "bg-white/[0.06] text-[var(--color-text-secondary)] border-white/[0.08] hover:bg-white/[0.1] hover:border-white/[0.14] hover:text-[var(--color-text-primary)]"
    : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)] border-transparent hover:bg-[rgb(237,237,237)] hover:border-black/5 hover:text-[rgb(20,20,22)]";
  return (
    <div
      className={classNames(
        "relative flex items-center gap-1.5 border-b px-2.5 py-1",
        isDark ? "border-white/[0.04]" : "border-black/[0.04]",
      )}
    >
      <span
        className={classNames(
          "flex-shrink-0 text-xs font-medium tracking-[0.08em]",
          isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-400",
        )}
      >
        {t("to", "To")}
      </span>

      <ScrollFade className="min-w-0 flex-1" innerClassName="w-full max-w-full" fadeWidth={20}>
        <div className="flex min-w-max items-center gap-1 transition-opacity">
          <div
            className={classNames(
              "flex items-center gap-1 transition-opacity",
              selectedGroupActorsHydrating ? "opacity-50 pointer-events-none" : "",
            )}
          >
            {["@all", "@foreman", "@peers"].map((token) => {
              const active = toTokens.includes(token);
              const target = popover.selectorTarget(token);
              return (
                <button
                  key={token}
                  className={classNames(
                    chipBaseClass,
                    active ? chipActiveClass : chipInactiveClass,
                  )}
                  onClick={() => onToggleRecipient(token)}
                  onMouseEnter={(event) => popover.show(target, event.currentTarget)}
                  onMouseLeave={popover.scheduleHide}
                  onFocus={(event) => popover.show(target, event.currentTarget)}
                  onBlur={popover.scheduleHide}
                  disabled={!selectedGroupId || busy === "send"}
                  aria-pressed={active}
                >
                  <span className="truncate">{token}</span>
                </button>
              );
            })}
            {actors.map((actor) => {
              const id = String(actor.id || "");
              if (!id) return null;
              const active = toTokens.includes(id);
              const target = popover.actorTarget(actor);
              return (
                <button
                  key={id}
                  className={classNames(
                    chipBaseClass,
                    active ? chipActiveClass : chipInactiveClass,
                  )}
                  onClick={() => onToggleRecipient(id)}
                  onMouseEnter={(event) => popover.show(target, event.currentTarget)}
                  onMouseLeave={popover.scheduleHide}
                  onFocus={(event) => popover.show(target, event.currentTarget)}
                  onBlur={popover.scheduleHide}
                  disabled={actorChipDisabled}
                  aria-pressed={active}
                >
                  <span className="truncate">{actor.title || id}</span>
                </button>
              );
            })}
          </div>
        </div>
      </ScrollFade>

      <RecipientPopover
        isDark={isDark}
        target={popover.visibleTarget}
        style={popover.style}
        onCancelHide={popover.cancelHide}
        onScheduleHide={popover.scheduleHide}
        onCopy={popover.copyIdentifier}
        onHide={popover.hide}
      />

      {toTokens.length > 0 && (
        <button
          className={classNames(
            "flex-shrink-0 h-7 w-7 rounded-full flex items-center justify-center transition-colors opacity-50 hover:opacity-100",
            isDark
              ? "text-[var(--color-text-tertiary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
              : "text-gray-400 hover:bg-black/5 hover:text-gray-700",
          )}
          onClick={onClearRecipients}
          disabled={busy === "send"}
          aria-label={t("clearRecipients")}
          title={t("clearRecipients")}
        >
          <CloseIcon size={12} />
        </button>
      )}
    </div>
  );
}
