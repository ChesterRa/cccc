import { useTranslation } from "react-i18next";
import type { TextScale, Theme } from "../../types";
import { normalizeLanguageCode } from "../../i18n/languages";
import { getTextScaleLabel, TEXT_SCALE_OPTIONS } from "../../utils/textScale";

export interface AppearancePreferencesProps {
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
}

// Native choices keep keyboard and touch selection inside the containing menu.
export function AppearancePreferences({
  theme,
  textScale,
  onThemeChange,
  onTextScaleChange,
}: AppearancePreferencesProps) {
  const { t, i18n } = useTranslation(["layout", "common"]);
  const row = "flex min-h-11 items-center justify-between gap-3 px-2 text-sm";
  const select =
    "min-w-0 w-36 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-2 py-1.5 text-sm text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2";

  return (
    <fieldset className="min-w-0" data-appearance-preferences>
      <legend className="px-2 pb-1 text-xs font-medium text-[var(--color-text-tertiary)]">
        {t("appearanceSection")}
      </legend>
      <label className={row}>
        <span className="shrink-0">{t("themeLabel")}</span>
        <select
          className={select}
          value={theme}
          onChange={(event) => onThemeChange(event.target.value as Theme)}
        >
          <option value="system">{t("themeSystem")}</option>
          <option value="light">{t("themeLight")}</option>
          <option value="dark">{t("themeDark")}</option>
        </select>
      </label>
      <label className={row}>
        <span className="shrink-0">{t("textSizeLabel")}</span>
        <select
          className={select}
          value={textScale}
          onChange={(event) => onTextScaleChange(Number(event.target.value) as TextScale)}
        >
          {TEXT_SCALE_OPTIONS.map((scale) => (
            <option key={scale} value={scale}>
              {getTextScaleLabel(scale)}
            </option>
          ))}
        </select>
      </label>
      <label className={row}>
        <span className="shrink-0">{t("common:language")}</span>
        <select
          className={select}
          value={normalizeLanguageCode(i18n.resolvedLanguage ?? i18n.language)}
          onChange={(event) => void i18n.changeLanguage(event.target.value)}
        >
          <option value="en">English</option>
          <option value="zh">中文</option>
          <option value="ja">日本語</option>
        </select>
      </label>
    </fieldset>
  );
}
