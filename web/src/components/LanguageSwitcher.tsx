import { useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { classNames } from "../utils/classNames";
import {
  LANGUAGE_NAME_KEY,
  LANGUAGE_SHORT_LABEL,
  SUPPORTED_LANGUAGES,
  normalizeLanguageCode,
  LanguageCode,
} from "../i18n/languages";

interface LanguageSwitcherProps {
  isDark: boolean;
  showLabel?: boolean;
  variant?: "default" | "row" | "rail";
  className?: string;
}

const LANGUAGE_NATIVE_NAME: Record<LanguageCode, string> = {
  en: "English",
  zh: "中文",
  ja: "日本語",
};

export function LanguageSwitcher({ isDark: _isDark, showLabel = false, variant = "default", className }: LanguageSwitcherProps) {
  const { i18n, t } = useTranslation(["layout", "common"]);
  const [isOpen, setIsOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelStyle, setPanelStyle] = useState<React.CSSProperties>({});

  const currentLang = normalizeLanguageCode(i18n.resolvedLanguage ?? i18n.language);
  const currentLanguageLabel = t(`common:${LANGUAGE_NAME_KEY[currentLang]}`);

  const close = useCallback(() => setIsOpen(false), []);

  const isRow = variant === "row";
  const isRail = variant === "rail";
  // Position the panel relative to the trigger button
  useEffect(() => {
    if (!isOpen || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    if (showLabel || isRow) {
      // Mobile: open upward, centered on button
      setPanelStyle({
        position: "fixed",
        bottom: window.innerHeight - rect.top + 8,
        left: rect.left,
        width: rect.width,
      });
    } else {
      // Desktop: open downward, right-aligned
      setPanelStyle({
        position: "fixed",
        top: rect.bottom + 8,
        right: window.innerWidth - rect.right,
      });
    }
  }, [isOpen, isRow, showLabel]);

  // Click outside & ESC to close
  useEffect(() => {
    if (!isOpen) return;
    const onPointerDown = (e: MouseEvent | TouchEvent) => {
      const target = e.target;
      if (!(target instanceof Node)) return;
      if (triggerRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      close();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("touchstart", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("touchstart", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [isOpen, close]);

  const selectLanguage = (lang: LanguageCode) => {
    void i18n.changeLanguage(lang);
    close();
  };

  const panel = isOpen
    ? createPortal(
        <div
          ref={panelRef}
          className="z-[9999] py-1.5 rounded-xl glass-modal min-w-[160px] shadow-lg animate-scale-in origin-top-right"
          style={panelStyle}
          role="listbox"
          aria-label={t("common:language")}
        >
          {SUPPORTED_LANGUAGES.map((lang) => {
            const isActive = lang === currentLang;
            return (
              <button
                key={lang}
                role="option"
                aria-selected={isActive}
                onClick={() => selectLanguage(lang)}
                className={classNames(
                  "w-full flex items-center gap-3 px-3 py-2 text-[13px] transition-colors relative",
                  isActive
                    ? "text-[var(--color-text-primary)]"
                    : "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-black/[.03] dark:hover:bg-[var(--glass-tab-bg-hover)]"
                )}
              >
                {isActive && (
                  <span
                    className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-4 rounded-r-full bg-gray-900 dark:bg-white"
                  />
                )}
                <span className={classNames(
                  "w-7 text-center text-xs font-mono shrink-0 tracking-wide",
                  isActive ? "font-bold" : "font-normal"
                )}>
                  {LANGUAGE_SHORT_LABEL[lang]}
                </span>
                <span className={classNames(
                  "text-sm",
                  isActive ? "font-medium" : "font-normal"
                )}>
                  {LANGUAGE_NATIVE_NAME[lang]}
                </span>
              </button>
            );
          })}
        </div>,
        document.body
      )
    : null;

  return (
    <div className={classNames((showLabel || isRow) && "w-full")}>
      <button
        ref={triggerRef}
        onClick={() => setIsOpen((v) => !v)}
        className={classNames(
          "transition-all duration-150 font-semibold tracking-wide select-none",
          isRow
            ? "w-full flex items-center justify-between gap-3 px-3.5 py-3 rounded-xl text-sm min-h-[48px] active:scale-[0.98]"
            : isRail
            ? "flex items-center justify-center h-9 w-9 min-w-[36px] min-h-[36px] rounded-[14px] text-[11px] shrink-0 border border-transparent bg-transparent active:scale-[0.95]"
            : showLabel
            ? "w-full flex items-center justify-center gap-2 px-3 py-3 text-sm rounded-2xl min-h-[52px] glass-btn active:scale-[0.98]"
            : "flex items-center justify-center w-11 h-11 min-w-[44px] min-h-[44px] rounded-xl text-xs shrink-0 glass-btn active:scale-[0.95]",
          isRow
            ? "text-[var(--color-text-primary)] hover:bg-black/5 dark:hover:bg-white/6"
            : isRail
            ? "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
            : "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
          className
        )}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        aria-label={t("layout:currentLanguage", { language: currentLanguageLabel })}
      >
        {isRow ? (
          <>
            <span className="truncate font-medium">{t("common:language")}</span>
            <span className="truncate text-[13px] font-medium text-[var(--color-text-tertiary)]">
              {currentLanguageLabel}
            </span>
          </>
        ) : (
          <>
            {LANGUAGE_SHORT_LABEL[currentLang]}
            {showLabel && <span className="truncate font-medium">{currentLanguageLabel}</span>}
          </>
        )}
      </button>
      {panel}
    </div>
  );
}
