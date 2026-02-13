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
  className?: string;
}

const LANGUAGE_NATIVE_NAME: Record<LanguageCode, string> = {
  en: "English",
  zh: "中文",
  ja: "日本語",
};

export function LanguageSwitcher({ isDark, showLabel = false, className }: LanguageSwitcherProps) {
  const { i18n, t } = useTranslation(["layout", "common"]);
  const [isOpen, setIsOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelStyle, setPanelStyle] = useState<React.CSSProperties>({});

  const currentLang = normalizeLanguageCode(i18n.resolvedLanguage ?? i18n.language);
  const currentLanguageLabel = t(`common:${LANGUAGE_NAME_KEY[currentLang]}`);

  const close = useCallback(() => setIsOpen(false), []);

  // Position the panel relative to the trigger button
  useEffect(() => {
    if (!isOpen || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    if (showLabel) {
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
  }, [isOpen, showLabel]);

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
                    ? isDark
                      ? "text-white"
                      : "text-gray-900"
                    : isDark
                      ? "text-slate-400 hover:text-white hover:bg-white/5"
                      : "text-gray-500 hover:text-gray-900 hover:bg-black/[.03]"
                )}
              >
                {isActive && (
                  <span
                    className={classNames(
                      "absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-4 rounded-r-full",
                      isDark ? "bg-white" : "bg-gray-900"
                    )}
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
    <div className={classNames(showLabel && "w-full")}>
      <button
        ref={triggerRef}
        onClick={() => setIsOpen((v) => !v)}
        className={classNames(
          "transition-all font-semibold tracking-wide select-none",
          showLabel
            ? "w-full flex items-center justify-center gap-2 px-3 py-3 text-sm rounded-2xl min-h-[52px] glass-btn"
            : "flex items-center justify-center w-9 h-9 rounded-xl text-xs glass-btn",
          isDark ? "text-slate-300 hover:text-white" : "text-gray-500 hover:text-gray-900",
          className
        )}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        aria-label={t("layout:currentLanguage", { language: currentLanguageLabel })}
      >
        {LANGUAGE_SHORT_LABEL[currentLang]}
        {showLabel && <span className="truncate font-medium">{currentLanguageLabel}</span>}
      </button>
      {panel}
    </div>
  );
}
