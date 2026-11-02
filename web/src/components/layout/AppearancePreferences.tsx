import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TextScale, Theme } from "../../types";
import { useUIStore } from "../../stores/useUIStore";
import { normalizeLanguageCode } from "../../i18n/languages";
import { getTextScaleLabel, TEXT_SCALE_OPTIONS } from "../../utils/textScale";
import {
  setUiStylePrefs,
  useSystemReducedMotion,
  useUiStylePrefs,
  type LcdBodyFont,
  type LcdMotion,
  type LcdWidth,
} from "../../utils/uiStyle";
import { SegmentedChoice, type SegmentedOption } from "../ui/segmented-choice";
import { ChevronDownIcon } from "../Icons";

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
  className = "",
}: {
  name:
    | "theme"
    | "lcdBody"
    | "lcdWidth"
    | "lcdMotion"
    | "textScale"
    | "language"
    | "pendingPrompts";
  label: string;
  value: Value;
  options: SegmentedOption<Value>[];
  onChange: (value: Value) => void;
  className?: string;
}) {
  const labelId = useId();
  return (
    <div className={`flex flex-col gap-1.5 px-2 pt-1 pb-2 ${className}`}>
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
  const uiStyle = useUiStylePrefs();
  const systemReducedMotion = useSystemReducedMotion();
  const showPendingPrompts = useUIStore((state) => state.showPendingPrompts);
  const setShowPendingPrompts = useUIStore((state) => state.setShowPendingPrompts);
  // Theme and language stay in view; the choices people set once and leave fold behind
  // one key after them, so opening it moves nothing above, and every opening of the menu
  // starts folded.
  const [moreOpen, setMoreOpen] = useState(false);
  const moreId = useId();
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
      <button
        type="button"
        className="flex min-h-10 w-full items-center gap-2.5 rounded-[10px] px-2 text-left text-[13px] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 pointer-coarse:min-h-11"
        aria-expanded={moreOpen}
        aria-controls={moreId}
        onClick={() => setMoreOpen((open) => !open)}
        data-appearance-more
      >
        <span className="flex-1">{t("moreAppearanceSettings")}</span>
        <ChevronDownIcon
          size={14}
          aria-hidden="true"
          className={`shrink-0 text-[var(--color-text-tertiary)] ${moreOpen ? "rotate-180" : ""}`}
        />
      </button>
      <div id={moreId} hidden={!moreOpen}>
        <AppearanceChoice<LcdBodyFont>
          name="lcdBody"
          label={t("lcdBodyLabel")}
          value={uiStyle.lcdBody}
          onChange={(lcdBody) => setUiStylePrefs({ lcdBody })}
          options={[
            { value: "mono", label: t("lcdBodyMono") },
            { value: "pixel", label: t("lcdBodyPixel") },
          ]}
        />
        {/* Below 768px messages already use the full width, so the choice would change nothing. */}
        <AppearanceChoice<LcdWidth>
          name="lcdWidth"
          label={t("lcdWidthLabel")}
          value={uiStyle.lcdWidth}
          onChange={(lcdWidth) => setUiStylePrefs({ lcdWidth })}
          options={[
            { value: "column", label: t("lcdWidthColumn") },
            { value: "wide", label: t("lcdWidthWide") },
            { value: "full", label: t("lcdWidthFull") },
          ]}
          className="max-md:hidden"
        />
        <AppearanceChoice<LcdMotion>
          name="lcdMotion"
          label={t("lcdMotionLabel")}
          value={uiStyle.lcdMotion}
          onChange={(lcdMotion) => setUiStylePrefs({ lcdMotion })}
          options={[
            { value: "on", label: t("lcdMotionOn") },
            { value: "off", label: t("lcdMotionOff") },
          ]}
          className={systemReducedMotion ? "pb-1" : ""}
        />
        {/* The system's reduced-motion setting stills it whatever is chosen; say so
              where the choice is made, without changing the stored choice. */}
        {systemReducedMotion ? (
          <p
            className="px-2 pb-2 text-xs text-[var(--color-text-secondary)]"
            data-lcd-motion-system
          >
            {t("lcdMotionSystemStill")}
          </p>
        ) : null}
        <AppearanceChoice
          name="pendingPrompts"
          label={t("pendingPromptsLabel")}
          value={showPendingPrompts ? "show" : "off"}
          onChange={(value) => setShowPendingPrompts(value === "show")}
          options={[
            { value: "show", label: t("pendingPromptsShow") },
            { value: "off", label: t("pendingPromptsOff") },
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
      </div>
    </fieldset>
  );
}
