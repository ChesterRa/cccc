import { useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { classNames } from "../utils/classNames";
import { GlobeIcon, ChevronDownIcon } from "./Icons";
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
      // Mobile: open upward from button
      setPanelStyle({
        position: "fixed",
        bottom: window.innerHeight - rect.top + 6,
        left: rect.left,
        width: rect.width,
      });
    } else {
      // Desktop: open downward, right-aligned
      setPanelStyle({
        position: "fixed",
        top: rect.bottom + 6,
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
          className={classNames(
            "z-[9999] py-1 rounded-xl glass-modal min-w-[180px] shadow-lg",
            "animate-fade-in"
          )}
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
                  "w-full flex items-center gap-3 px-3 py-2.5 text-sm transition-colors",
                  isActive
                    ? isDark
                      ? "bg-white/10 text-white"
                      : "bg-black/5 text-gray-900"
                    : isDark
                      ? "text-slate-300 hover:bg-white/5 hover:text-white"
                      : "text-gray-600 hover:bg-black/[.03] hover:text-gray-900"
                )}
              >
                <span className="w-4 text-center text-xs shrink-0">
                  {isActive ? "✓" : ""}
                </span>
                <span className="font-medium">{LANGUAGE_NATIVE_NAME[lang]}</span>
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
          "rounded-xl transition-all glass-btn font-medium",
          showLabel
            ? "w-full flex items-center justify-center gap-2 px-3 py-3 text-sm min-h-[52px]"
            : "flex items-center justify-center gap-1.5 px-2.5 h-9 min-h-[44px] text-xs",
          isDark ? "text-slate-300 hover:text-white" : "text-gray-600 hover:text-gray-900",
          className
        )}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        aria-label={t("layout:currentLanguage", { language: currentLanguageLabel })}
      >
        <GlobeIcon size={showLabel ? 18 : 16} />
        <span
          className={classNames(
            "inline-flex items-center justify-center rounded-md px-1.5 py-0.5 text-[11px] font-semibold tracking-wide",
            isDark ? "bg-white/10 text-slate-100" : "bg-black/5 text-gray-700"
          )}
        >
          {LANGUAGE_SHORT_LABEL[currentLang]}
        </span>
        {showLabel && <span className="truncate">{currentLanguageLabel}</span>}
        <ChevronDownIcon
          size={12}
          className={classNames("transition-transform duration-200", isOpen && "rotate-180")}
        />
      </button>
      {panel}
    </div>
  );
}
