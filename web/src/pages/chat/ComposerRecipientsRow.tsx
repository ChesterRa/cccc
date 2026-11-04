import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { CloseIcon } from "../../components/Icons";
import { ScrollFade } from "../../components/ScrollFade";
import type { Actor } from "../../types";
import { classNames } from "../../utils/classNames";
import { CHET_SELECTOR } from "../../utils/chetIdentity";
import { RecipientPopover } from "./RecipientPopover";
import { useRecipientPopover } from "./useRecipientPopover";

interface ComposerRecipientsRowProps {
  isDark: boolean;
  isSmallScreen: boolean;
  selectedGroupId: string;
  busy: string;
  actors: Actor[];
  allowChet?: boolean;
  selectedGroupActorsHydrating?: boolean;
  toTokens: string[];
  unavailableRecipients?: string[];
  onToggleRecipient: (token: string) => void;
  onClearRecipients: () => void;
}

export function ComposerRecipientsRow({
  isDark,
  isSmallScreen,
  selectedGroupId,
  busy,
  actors,
  allowChet = false,
  selectedGroupActorsHydrating,
  toTokens,
  unavailableRecipients = [],
  onToggleRecipient,
  onClearRecipients,
}: ComposerRecipientsRowProps) {
  const { t } = useTranslation("chat");
  const popover = useRecipientPopover({ isSmallScreen });
  const lastSelected = useRef<HTMLButtonElement | null>(null);
  const fadeWidth = 20;
  useLayoutEffect(() => {
    const button = lastSelected.current;
    lastSelected.current = null;
    if (!button?.isConnected || button.getAttribute("aria-pressed") !== "true") return;
    const scrollPort = button.closest<HTMLElement>(".composer-recipient-scroll");
    if (!scrollPort || scrollPort.scrollWidth <= scrollPort.clientWidth) return;
    const viewport = scrollPort.getBoundingClientRect();
    const target = button.getBoundingClientRect();
    // A newly shown clear button can shrink this viewport after the click.
    // Move only this scroll port, and leave room for its faded edge.
    if (target.left < viewport.left) {
      const margin =
        target.width > viewport.width
          ? fadeWidth
          : Math.min(fadeWidth, viewport.width - target.width);
      scrollPort.scrollLeft -= viewport.left - target.left + margin;
    } else if (target.width <= viewport.width && target.right > viewport.right) {
      const margin = Math.min(fadeWidth, viewport.width - target.width);
      scrollPort.scrollLeft += target.right - viewport.right + margin;
    }
  }, [toTokens]);
  const actorChipDisabled = !selectedGroupId || busy === "send" || !!selectedGroupActorsHydrating;
  const chipBaseClass =
    "touch-target-sm flex h-6 flex-shrink-0 items-center justify-center whitespace-nowrap rounded-lg border px-2 text-xs font-medium leading-none transition-all sm:px-2.5 sm:text-xs";
  const chipActiveClass =
    "border-[var(--glass-accent-border)] bg-[var(--glass-accent-bg)] text-[var(--color-text-primary)] shadow-none";
  const chipInactiveClass =
    "border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]";
  return (
    <div
      role="group"
      aria-label={t("to", "To")}
      data-composer-recipient-row
      className="relative flex items-center gap-2 px-3 py-1"
    >
      <ScrollFade
        className="min-w-0 flex-1"
        innerClassName="w-full max-w-full composer-recipient-scroll"
        fadeWidth={fadeWidth}
      >
        <div className="flex min-w-max items-center gap-1 transition-opacity">
          <div
            className={classNames(
              "flex items-center gap-1 transition-opacity",
              selectedGroupActorsHydrating ? "opacity-50 pointer-events-none" : "",
            )}
          >
            {unavailableRecipients.map((token) => (
              <button
                key={`unavailable:${token}`}
                className={`${chipBaseClass} border-[var(--color-danger)] text-[var(--color-danger)]`}
                onClick={() => onToggleRecipient(token)}
                disabled={actorChipDisabled}
                aria-label={t("removeUnavailableRecipient", { name: token })}
                title={t("removeUnavailableRecipient", { name: token })}
                data-unavailable-recipient
              >
                <span className="max-w-48 truncate">
                  {t("unavailableRecipient", { name: token })}
                </span>
                <CloseIcon size={12} aria-hidden="true" />
              </button>
            ))}
            {[
              "@all",
              "@foreman",
              "@peers",
              ...(allowChet || toTokens.includes(CHET_SELECTOR) ? [CHET_SELECTOR] : []),
            ].map((token) => {
              const active = toTokens.includes(token);
              const target = popover.selectorTarget(token);
              return (
                <button
                  key={token}
                  className={classNames(
                    chipBaseClass,
                    active ? chipActiveClass : chipInactiveClass,
                  )}
                  onClick={(event) => {
                    lastSelected.current = active ? null : event.currentTarget;
                    onToggleRecipient(token);
                  }}
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
                  onClick={(event) => {
                    lastSelected.current = active ? null : event.currentTarget;
                    onToggleRecipient(id);
                  }}
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
        anchorRef={popover.anchorRef}
        isDark={isDark}
        target={popover.visibleTarget}
        style={popover.style}
        onCancelHide={popover.cancelHide}
        onScheduleHide={popover.scheduleHide}
        onCopy={popover.copyIdentifier}
        onHide={popover.hide}
      />

      {(toTokens.length > 0 || unavailableRecipients.length > 0) && (
        <button
          className={classNames(
            "touch-target-sm flex-shrink-0 h-7 w-7 rounded-md flex items-center justify-center transition-colors opacity-50 hover:opacity-100",
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
