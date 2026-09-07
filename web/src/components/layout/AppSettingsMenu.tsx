import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AccountIcon, SettingsIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { AppearancePreferences, type AppearancePreferencesProps } from "./AppearancePreferences";

export function AppSettingsMenu({
  canAccessAccount,
  canOpenSettings,
  onOpenAccount,
  onOpenSettings,
  ...appearance
}: AppearancePreferencesProps & {
  canAccessAccount: boolean;
  canOpenSettings: boolean;
  onOpenAccount: () => void;
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation("layout");
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const openingDialog = useRef(false);
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
        openingDialog.current = false;
        setOpen(value);
      }}
    >
      <PopoverTrigger asChild>
        <IconButton
          ref={triggerRef}
          type="button"
          variant="ghost"
          label={t("settingsAndMore")}
          className="text-[var(--color-text-secondary)]"
          data-app-settings-trigger
        >
          <SettingsIcon size={18} />
        </IconButton>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={8}
        collisionPadding={12}
        className="w-72 max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-xl p-2"
        aria-label={t("settingsAndMore")}
        data-app-settings-menu
        onEscapeKeyDown={(event) => event.stopPropagation()}
        onCloseAutoFocus={(event) => {
          if (openingDialog.current) event.preventDefault();
        }}
      >
        <AppearancePreferences {...appearance} />
        <div className="my-2 border-t border-[var(--glass-border-subtle)]" />
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
