import { useTranslation } from "react-i18next";

import type { TextScale } from "../types";
import { classNames } from "../utils/classNames";
import { getNextTextScale, getTextScaleLabel } from "../utils/textScale";
import { TextSizeIcon } from "./Icons";

interface TextScaleSwitcherProps {
  textScale: TextScale;
  onTextScaleChange: (scale: TextScale) => void;
  variant?: "rail" | "row";
  className?: string;
}

export function TextScaleSwitcher({
  textScale,
  onTextScaleChange,
  variant = "rail",
  className,
}: TextScaleSwitcherProps) {
  const { t } = useTranslation("layout");
  const isRow = variant === "row";
  const currentScaleLabel = getTextScaleLabel(textScale);
  const nextScale = getNextTextScale(textScale);
  const nextScaleLabel = getTextScaleLabel(nextScale);

  const railButton = (
    <button
      type="button"
      onClick={() => onTextScaleChange(nextScale)}
      className={classNames(
        "flex items-center justify-center h-9 w-9 min-w-[36px] min-h-[36px] rounded-[14px] shrink-0 border border-transparent bg-transparent text-[var(--color-text-secondary)] transition-all duration-150 active:scale-[0.95] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
        className,
      )}
      title={t("switchTextSize", { percent: nextScaleLabel })}
      aria-label={`${t("currentTextSize", { percent: currentScaleLabel })}. ${t("switchTextSize", { percent: nextScaleLabel })}`}
    >
      <TextSizeIcon size={17} />
    </button>
  );

  if (!isRow) return railButton;

  return (
    <div className={classNames("w-full", className)}>
      <button
        type="button"
        onClick={() => onTextScaleChange(nextScale)}
        className="flex w-full items-center justify-start gap-3 rounded-xl px-3.5 py-3 text-sm min-h-[48px] text-[var(--color-text-primary)] transition-all duration-150 active:scale-[0.98] hover:bg-black/5 dark:hover:bg-white/6"
        title={t("switchTextSize", { percent: nextScaleLabel })}
        aria-label={`${t("currentTextSize", { percent: currentScaleLabel })}. ${t("switchTextSize", { percent: nextScaleLabel })}`}
      >
        <span className="flex min-w-0 items-center gap-3 truncate font-medium">
          <TextSizeIcon size={19} />
          <span className="truncate">{t("textSizeLabel")}</span>
        </span>
      </button>
    </div>
  );
}
