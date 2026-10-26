import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon } from "../../components/Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import type { ChatSenderFilter as SenderFilter } from "../../stores/useUIStore";
import type { Actor } from "../../types";
import { classNames } from "../../utils/classNames";

export function ChatSenderFilter({
  value,
  actors,
  onChange,
}: {
  value: SenderFilter;
  actors: Actor[];
  onChange: (value: SenderFilter) => void;
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const filtered = value.length > 0;
  const options = [
    { id: "user", label: t("senderUser"), foreman: false },
    ...[...actors]
      .sort((a, b) => Number(b.role === "foreman") - Number(a.role === "foreman"))
      .map((actor) => ({
        id: actor.id,
        label: actor.title || actor.id,
        foreman: actor.role === "foreman",
      })),
  ];
  // Keep selected historical identities available to uncheck after an Actor is removed.
  for (const id of value) {
    if (!options.some((option) => option.id === id))
      options.push({ id, label: id, foreman: false });
  }
  const triggerLabel =
    value.length === 1
      ? options.find((option) => option.id === value[0])!.label
      : filtered
        ? t("senderCount", { count: value.length })
        : t("senderLabel");

  return (
    <div className="flex h-11 md:h-9 min-w-0 shrink-0 items-center gap-1" data-chat-sender-filter>
      <Popover open={open} onOpenChange={setOpen} modal={false}>
        <PopoverTrigger asChild>
          <button
            ref={trigger}
            type="button"
            aria-label={filtered ? `${t("senderFilter")}: ${triggerLabel}` : t("senderFilter")}
            className={classNames(
              "flex h-11 md:h-9 min-w-0 max-w-[min(16rem,65vw)] items-center gap-2 rounded-lg px-2 text-[13px] leading-5 focus-visible:outline-2 focus-visible:outline-offset-2",
              filtered
                ? "font-semibold text-[var(--color-text-primary)]"
                : "font-medium text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]",
            )}
          >
            <span className="truncate">{triggerLabel}</span>
            <ChevronDownIcon size={14} className="shrink-0" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          ref={content}
          aria-label={t("senderFilter")}
          align="start"
          sideOffset={4}
          collisionPadding={12}
          className="flex w-64 max-w-[calc(100vw-24px)] max-h-[min(18rem,var(--radix-popover-content-available-height))] flex-col rounded-lg !bg-[var(--color-bg-primary)] !backdrop-blur-none p-1"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            const panel = content.current;
            const input =
              panel?.querySelector<HTMLInputElement>("input:checked") ??
              panel?.querySelector<HTMLInputElement>("input");
            input?.focus({ preventScroll: true });
            // Reveal the whole row after Radix measures the available popup height.
            requestAnimationFrame(() => {
              if (panel?.dataset.state === "open" && document.activeElement === input) {
                input?.closest("label")?.scrollIntoView({ block: "nearest" });
              }
            });
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            const closingPanel = event.currentTarget;
            if (
              document.activeElement === document.body ||
              (closingPanel instanceof HTMLElement && closingPanel.contains(document.activeElement))
            ) {
              trigger.current?.focus();
            }
          }}
          onEscapeKeyDown={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
            trigger.current?.focus();
          }}
          onKeyDown={(event) => {
            if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
            const items = Array.from(
              content.current?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]') ?? [],
            );
            const current = items.indexOf(document.activeElement as HTMLInputElement);
            // Leave the reset button's native keyboard behavior intact.
            if (current < 0) return;
            event.preventDefault();
            event.stopPropagation();
            const index =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? items.length - 1
                  : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
            items[index]?.focus();
            items[index]?.closest("label")?.scrollIntoView({ block: "nearest" });
          }}
        >
          <div className="flex shrink-0 items-center justify-between gap-3 px-3 py-1 text-xs">
            <span className="font-medium text-[var(--color-text-secondary)]">
              {t("senderLabel")}
            </span>
            <button
              type="button"
              disabled={!filtered}
              onClick={() => onChange([])}
              className="min-h-8 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] focus-visible:outline-2 disabled:opacity-40"
            >
              {t("senderShowAll")}
            </button>
          </div>
          <div
            role="group"
            aria-label={t("senderFilter")}
            className="min-h-0 overflow-y-auto overscroll-contain"
          >
            {options.map((option) => (
              <label
                key={option.id}
                title={option.label}
                className="flex min-h-[44px] cursor-pointer items-center gap-3 rounded-md px-3 text-sm text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg)] focus-within:bg-[var(--glass-tab-bg)]"
              >
                <input
                  type="checkbox"
                  value={option.id}
                  checked={value.includes(option.id)}
                  onChange={(event) =>
                    onChange(
                      event.target.checked
                        ? [...value, option.id]
                        : value.filter((id) => id !== option.id),
                    )
                  }
                  className="size-4 shrink-0 accent-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2"
                />
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
                {option.foreman ? (
                  <span className="shrink-0 rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-text-tertiary)]">
                    {t("senderForeman")}
                  </span>
                ) : null}
              </label>
            ))}
          </div>
        </PopoverContent>
      </Popover>
      {filtered ? (
        <button
          type="button"
          onClick={() => {
            onChange([]);
            trigger.current?.focus();
          }}
          aria-label={t("clearSenderFilter")}
          title={t("clearSenderFilter")}
          className="flex size-9 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
        >
          <span aria-hidden="true">×</span>
        </button>
      ) : null}
    </div>
  );
}
