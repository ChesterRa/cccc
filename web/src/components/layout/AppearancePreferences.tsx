import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TextScale, Theme } from "../../types";
import { normalizeLanguageCode } from "../../i18n/languages";
import { getTextScaleLabel, TEXT_SCALE_OPTIONS } from "../../utils/textScale";
import { ChevronDownIcon } from "../Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export interface AppearancePreferencesProps {
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
}

function AppearanceSelect<Value extends string | number>({
  name,
  label,
  value,
  options,
  onChange,
  ariaLabel,
}: {
  name: "theme" | "textScale" | "language";
  label: string;
  value: Value;
  options: { value: Value; label: string }[];
  onChange: (value: Value) => void;
  ariaLabel: string;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  return (
    <div className="flex min-h-11 items-center justify-between gap-3 px-2 text-sm">
      <span className="shrink-0">{label}</span>
      <Popover open={open} onOpenChange={setOpen} modal={false}>
        <PopoverTrigger asChild>
          <button
            ref={trigger}
            data-appearance-select={name}
            data-value={value}
            type="button"
            aria-label={ariaLabel}
            aria-haspopup="menu"
            className="flex min-h-[44px] w-36 min-w-0 items-center justify-between gap-2 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-2 text-sm text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2"
          >
            <span className="truncate">
              {options.find((option) => option.value === value)?.label}
            </span>
            <ChevronDownIcon size={14} className="shrink-0" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          ref={menu}
          data-appearance-menu={name}
          role="menu"
          aria-label={ariaLabel}
          align="end"
          sideOffset={4}
          collisionPadding={12}
          className="w-40 max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-lg !bg-[var(--color-bg-primary)] p-1"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            menu.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            // Closing animation cleanup may run after another choice has opened.
            // Restore only abandoned focus, never steal it from the next menu.
            if (
              document.activeElement === document.body ||
              menu.current?.contains(document.activeElement)
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
            if (event.key === "Tab") {
              event.preventDefault();
              event.stopPropagation();
              setOpen(false);
              return;
            }
            const keys = ["ArrowDown", "ArrowUp", "Home", "End"];
            if (!keys.includes(event.key)) return;
            event.preventDefault();
            event.stopPropagation();
            const items = Array.from(
              menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') || [],
            );
            const current = items.indexOf(document.activeElement as HTMLButtonElement);
            const index =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? items.length - 1
                  : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
            items[index]?.focus();
          }}
        >
          {options.map((option) => (
            <button
              key={option.value}
              type="button"
              role="menuitemradio"
              data-value={option.value}
              aria-checked={option.value === value}
              tabIndex={-1}
              className="flex min-h-[44px] w-full items-center justify-between gap-2 rounded-md px-3 text-left text-sm text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg)] focus:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
                trigger.current?.focus();
              }}
            >
              <span>{option.label}</span>
              {option.value === value ? <span aria-hidden="true">✓</span> : null}
            </button>
          ))}
        </PopoverContent>
      </Popover>
    </div>
  );
}

export function AppearancePreferences({
  theme,
  textScale,
  onThemeChange,
  onTextScaleChange,
}: AppearancePreferencesProps) {
  const { t, i18n } = useTranslation(["layout", "common"]);
  return (
    <fieldset className="min-w-0" data-appearance-preferences>
      <legend className="px-2 pb-1 text-xs font-medium text-[var(--color-text-tertiary)]">
        {t("appearanceSection")}
      </legend>
      <AppearanceSelect<Theme>
        name="theme"
        label={t("themeLabel")}
        ariaLabel={t("themeLabel")}
        value={theme}
        onChange={onThemeChange}
        options={[
          { value: "system", label: t("themeSystem") },
          { value: "light", label: t("themeLight") },
          { value: "dark", label: t("themeDark") },
        ]}
      />
      <AppearanceSelect<TextScale>
        name="textScale"
        label={t("textSizeLabel")}
        ariaLabel={t("textSizeLabel")}
        value={textScale}
        onChange={onTextScaleChange}
        options={TEXT_SCALE_OPTIONS.map((scale) => ({
          value: scale,
          label: getTextScaleLabel(scale),
        }))}
      />
      <AppearanceSelect
        name="language"
        label={t("common:language")}
        ariaLabel={t("common:language")}
        value={normalizeLanguageCode(i18n.resolvedLanguage ?? i18n.language)}
        onChange={(language) => void i18n.changeLanguage(language)}
        options={[
          { value: "en", label: "English" },
          { value: "zh", label: "中文" },
          { value: "ja", label: "日本語" },
        ]}
      />
    </fieldset>
  );
}
