import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AccountIcon, ChevronRightIcon, SettingsIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { AppearancePreferences, type AppearancePreferencesProps } from "./AppearancePreferences";

export function AppSettingsMenu({
  canAccessAccount,
  accountLabel,
  canOpenSettings,
  onOpenAccount,
  onOpenSettings,
  ...appearance
}: AppearancePreferencesProps & {
  canAccessAccount: boolean;
  accountLabel?: string | null;
  canOpenSettings: boolean;
  onOpenAccount: () => void;
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation("layout");
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const openingDialog = useRef(false);
  const row =
    "flex min-h-10 w-full items-center gap-2.5 rounded-[10px] px-2 text-left text-sm text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg)] disabled:opacity-45 focus-visible:outline-2 pointer-coarse:min-h-11";
  const divider = <div className="my-1 border-t border-[var(--glass-border-subtle)]" />;
  const initial = accountLabel?.trim().charAt(0).toUpperCase();

  useEffect(() => {
    const trigger = triggerRef.current;
    if (!open || !trigger) return;
    // CSS owns the header breakpoint, including sidebar resizing and text scaling.
    // Close the portalled panel if its desktop trigger becomes hidden.
    const observer = new ResizeObserver(() => {
      if (!trigger.getClientRects().length) setOpen(false);
    });
    observer.observe(trigger);
    return () => observer.disconnect();
  }, [open]);

  const openDialog = (action: () => void) => {
    openingDialog.current = true;
    triggerRef.current?.focus();
    setOpen(false);
    action();
  };

  return (
    <Popover
      open={open}
      onOpenChange={(value) => {
        openingDialog.current = false;
        setOpen(value);
      }}
    >
      <PopoverTrigger asChild>
        <IconButton
          ref={triggerRef}
          type="button"
          variant="ghost"
          size="sm"
          label={t("settingsAndMore")}
          className="relative text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] data-[state=open]:bg-[var(--glass-tab-bg)] data-[state=open]:text-[var(--color-text-primary)]"
          data-app-settings-trigger
        >
          <SettingsIcon size={18} strokeWidth={1.25} />
        </IconButton>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={8}
        collisionPadding={12}
        className="w-[300px] max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-[14px] p-2"
        aria-label={t("settingsAndMore")}
        data-app-settings-menu
        onEscapeKeyDown={(event) => event.stopPropagation()}
        onCloseAutoFocus={(event) => {
          if (openingDialog.current) event.preventDefault();
        }}
      >
        {canAccessAccount ? (
          <>
            <button
              type="button"
              className={`${row} py-2`}
              onClick={() => openDialog(onOpenAccount)}
              data-app-settings-account
            >
              <span
                aria-hidden="true"
                className="flex size-8 shrink-0 items-center justify-center rounded-full bg-[var(--glass-tab-bg)] text-[13px] font-semibold text-[var(--color-text-secondary)]"
              >
                {initial || <AccountIcon size={16} />}
              </span>
              <span className="min-w-0 flex-1 text-left">
                <span className="block text-[13px] font-semibold">{t("account")}</span>
                {accountLabel ? (
                  <span
                    className="block truncate text-xs text-[var(--color-text-muted)]"
                    title={t("linkedAccount", { account: accountLabel })}
                  >
                    {accountLabel}
                  </span>
                ) : null}
              </span>
              <ChevronRightIcon
                size={14}
                aria-hidden="true"
                className="shrink-0 text-[var(--color-text-tertiary)]"
              />
            </button>
            {divider}
          </>
        ) : null}
        <AppearancePreferences {...appearance} />
        {divider}
        <button
          type="button"
          className={row}
          disabled={!canOpenSettings}
          onClick={() => openDialog(onOpenSettings)}
          data-app-settings-open-all
        >
          <SettingsIcon size={17} className="shrink-0 text-[var(--color-text-secondary)]" />
          <span className="flex-1">{t("allSettings")}</span>
          <ChevronRightIcon
            size={14}
            aria-hidden="true"
            className="shrink-0 text-[var(--color-text-tertiary)]"
          />
        </button>
      </PopoverContent>
    </Popover>
  );
}
