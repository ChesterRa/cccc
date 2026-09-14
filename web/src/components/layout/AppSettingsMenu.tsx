import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AccountIcon, BookmarkIcon, SettingsIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { AppearancePreferences, type AppearancePreferencesProps } from "./AppearancePreferences";
import { isMousePointer, useHoverIntent } from "../../hooks/useHoverIntent";

export type PresentationMenuEntry = {
  active: boolean;
  attention: boolean;
  disabled: boolean;
  onToggle: () => void;
};

export function AppSettingsMenu({
  canAccessAccount,
  canOpenSettings,
  onOpenAccount,
  onOpenSettings,
  presentation,
  ...appearance
}: AppearancePreferencesProps & {
  canAccessAccount: boolean;
  canOpenSettings: boolean;
  onOpenAccount: () => void;
  onOpenSettings: () => void;
  /** Absent where no group owns a presentation surface to show. */
  presentation?: PresentationMenuEntry;
}) {
  const { t } = useTranslation("layout");
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const openingDialog = useRef(false);
  const lastPointerType = useRef<string | null>(null);
  // A hover-opened menu must not steal keyboard focus from whatever the user was doing.
  const openedByHover = useRef(false);
  const { scheduleOpen, scheduleClose, cancel } = useHoverIntent((next) => {
    openedByHover.current = next;
    setOpen(next);
  });
  const row =
    "flex min-h-10 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm hover:bg-[var(--glass-tab-bg)] disabled:opacity-45 focus-visible:outline-2";

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
        openedByHover.current = false;
        setOpen(value);
      }}
    >
      <PopoverTrigger asChild>
        <IconButton
          ref={triggerRef}
          type="button"
          variant="ghost"
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
          {/* The presentation entry moved inside, so its attention marker has to surface here. */}
          {presentation?.attention ? (
            <span
              className="absolute right-1 top-1 h-2 w-2 rounded-full bg-cyan-500 dark:bg-cyan-200"
              data-app-settings-attention
              aria-hidden="true"
            />
          ) : null}
        </IconButton>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={8}
        collisionPadding={12}
        className="w-60 max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-xl p-2"
        aria-label={t("settingsAndMore")}
        data-app-settings-menu
        onPointerEnter={cancel}
        onPointerLeave={(event) => {
          if (isMousePointer(event)) scheduleClose();
        }}
        onOpenAutoFocus={(event) => {
          if (openedByHover.current) event.preventDefault();
        }}
        onEscapeKeyDown={(event) => event.stopPropagation()}
        onCloseAutoFocus={(event) => {
          if (openingDialog.current) event.preventDefault();
        }}
      >
        <AppearancePreferences {...appearance} />
        <div className="my-2 border-t border-[var(--glass-border-subtle)]" />
        {presentation ? (
          <button
            type="button"
            className={row}
            disabled={presentation.disabled}
            aria-pressed={presentation.active}
            data-app-settings-presentation
            onClick={() => {
              setOpen(false);
              presentation.onToggle();
            }}
          >
            <BookmarkIcon size={17} />
            {t("presentation")}
            {presentation.attention ? (
              <span
                className="ml-auto h-2 w-2 rounded-full bg-cyan-500 dark:bg-cyan-200"
                aria-hidden="true"
              />
            ) : null}
          </button>
        ) : null}
        {canAccessAccount ? (
          <button type="button" className={row} onClick={() => openDialog(onOpenAccount)}>
            <AccountIcon size={17} />
            {t("account")}
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
