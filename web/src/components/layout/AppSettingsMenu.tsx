import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AccountIcon, SettingsIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { AppearancePreferences, type AppearancePreferencesProps } from "./AppearancePreferences";
import { isMousePointer, useHoverIntent } from "../../hooks/useHoverIntent";

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
  const lastPointerType = useRef<string | null>(null);
  // A hover-opened menu must not steal keyboard focus from whatever the user was doing.
  const openedByHover = useRef(false);
  const { scheduleOpen, scheduleClose, cancel } = useHoverIntent((next) => {
    if (next && !open) openedByHover.current = true;
    setOpen(next);
  });
  const row =
    "flex min-h-9 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg)] disabled:opacity-45 focus-visible:outline-2 pointer-coarse:min-h-11 [&>svg]:text-[var(--color-text-secondary)]";

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
        // An explicit click or dismissal wins over any pending hover timer.
        cancel();
        openingDialog.current = false;
        if (value) openedByHover.current = false;
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
          className="relative text-[var(--color-text-secondary)]"
          data-app-settings-trigger
          onPointerDown={(event) => {
            lastPointerType.current = event.pointerType;
          }}
          onClick={(event) => {
            // A mouse already reaches this menu by hovering, so its click is the shortcut into
            // Settings. Touch, pen and keyboard (detail 0) keep the menu as their only way in.
            if (!canOpenSettings) return;
            if (event.detail === 0 || lastPointerType.current !== "mouse") return;
            event.preventDefault();
            cancel();
            setOpen(false);
            onOpenSettings();
          }}
          onPointerEnter={(event) => {
            if (isMousePointer(event)) scheduleOpen();
          }}
          onPointerLeave={(event) => {
            if (isMousePointer(event)) scheduleClose();
          }}
        >
          <SettingsIcon size={18} />
        </IconButton>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={8}
        collisionPadding={12}
        className="w-64 max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-xl p-2"
        aria-label={t("settingsAndMore")}
        data-app-settings-menu
        onPointerEnter={cancel}
        onPointerLeave={(event) => {
          if (isMousePointer(event)) scheduleClose();
        }}
        onFocusCapture={() => {
          openedByHover.current = false;
        }}
        onOpenAutoFocus={(event) => {
          if (openedByHover.current) event.preventDefault();
        }}
        onEscapeKeyDown={(event) => event.stopPropagation()}
        onCloseAutoFocus={(event) => {
          if (openingDialog.current || openedByHover.current) event.preventDefault();
        }}
      >
        <AppearancePreferences {...appearance} />
        <div className="my-1.5 border-t border-[var(--glass-border-subtle)]" />
        {canAccessAccount ? (
          <button type="button" className={row} onClick={() => openDialog(onOpenAccount)}>
            <AccountIcon size={17} />
            <span className="min-w-0 text-left">
              <span className="block">{t("account")}</span>
              {accountLabel ? (
                <span
                  className="block max-w-56 truncate text-xs text-[var(--color-text-muted)]"
                  title={t("linkedAccount", { account: accountLabel })}
                >
                  {accountLabel}
                </span>
              ) : null}
            </span>
          </button>
        ) : null}
        <button
          type="button"
          className={row}
          disabled={!canOpenSettings}
          onClick={() => openDialog(onOpenSettings)}
        >
          <SettingsIcon size={17} />
          {t("settingsButton")}
        </button>
      </PopoverContent>
    </Popover>
  );
}
