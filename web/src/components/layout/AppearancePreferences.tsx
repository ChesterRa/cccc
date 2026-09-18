import { useTranslation } from "react-i18next";
import type { TextScale, Theme } from "../../types";
import { normalizeLanguageCode } from "../../i18n/languages";
import { getTextScaleLabel, TEXT_SCALE_OPTIONS } from "../../utils/textScale";
import { SelectMenu, type SelectMenuOption } from "../ui/select-menu";

export interface AppearancePreferencesProps {
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
}

/** A labelled row wrapping the shared dropdown, so every preference lines up the same way. */
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
  options: SelectMenuOption<Value>[];
  onChange: (value: Value) => void;
  ariaLabel: string;
}) {
  return (
    <div className="flex min-h-9 items-center justify-between gap-3 px-2 text-sm pointer-coarse:min-h-11">
      <span className="shrink-0 text-[var(--color-text-secondary)]">{label}</span>
      {/* Lighter than the default form dropdown: this sits inside a menu, not a form. */}
      <SelectMenu<Value>
        value={value}
        options={options}
        onChange={onChange}
        ariaLabel={ariaLabel}
        className="min-h-8 w-28 rounded-md border-[var(--glass-border-subtle)] bg-transparent px-2.5 text-sm text-[var(--color-text-primary)] transition-colors hover:bg-[var(--glass-tab-bg)] pointer-coarse:min-h-10"
        contentClassName="w-28"
        triggerProps={{ "data-appearance-select": name }}
        contentProps={{ "data-appearance-menu": name }}
      />
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
      <legend className="px-2 pb-1.5 text-xs font-medium text-[var(--color-text-tertiary)]">
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
