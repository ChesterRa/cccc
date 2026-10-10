import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { TextScale, Theme } from "../../types";
import { normalizeLanguageCode } from "../../i18n/languages";
import { getTextScaleLabel, TEXT_SCALE_OPTIONS } from "../../utils/textScale";
import { SegmentedChoice, type SegmentedOption } from "../ui/segmented-choice";

export interface AppearancePreferencesProps {
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
}

/** One preference as a labelled segmented choice: every option visible, one tap applies it. */
function AppearanceChoice<Value extends string | number>({
  name,
  label,
  value,
  options,
  onChange,
}: {
  name: "theme" | "textScale" | "language";
  label: string;
  value: Value;
  options: SegmentedOption<Value>[];
  onChange: (value: Value) => void;
}) {
  const labelId = useId();
  return (
    <div className="flex flex-col gap-1.5 px-2 pt-1 pb-2">
      <span id={labelId} className="text-[13px] text-[var(--color-text-secondary)]">
        {label}
      </span>
      <SegmentedChoice<Value>
        labelledBy={labelId}
        value={value}
        options={options}
        onChange={onChange}
        data-appearance-choice={name}
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
      <legend className="px-2 pt-1.5 pb-1 text-xs font-semibold text-[var(--color-text-tertiary)]">
        {t("appearanceSection")}
      </legend>
      <AppearanceChoice<Theme>
        name="theme"
        label={t("themeLabel")}
        value={theme}
        onChange={onThemeChange}
        options={[
          { value: "light", label: t("themeLight") },
          { value: "dark", label: t("themeDark") },
          { value: "system", label: t("themeSystem") },
        ]}
      />
      <AppearanceChoice<TextScale>
        name="textScale"
        label={t("textSizeLabel")}
        value={textScale}
        onChange={onTextScaleChange}
        options={TEXT_SCALE_OPTIONS.map((scale) => ({
          value: scale,
          label: getTextScaleLabel(scale),
        }))}
      />
      <AppearanceChoice
        name="language"
        label={t("common:language")}
        value={normalizeLanguageCode(i18n.resolvedLanguage ?? i18n.language)}
        onChange={(language) => void i18n.changeLanguage(language)}
        options={[
          { value: "zh", label: "中文" },
          { value: "en", label: "English" },
          { value: "ja", label: "日本語" },
        ]}
      />
    </fieldset>
  );
}
