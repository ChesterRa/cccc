import { useTranslation } from "react-i18next";
import { classNames } from "../utils/classNames";
import { GlobeIcon } from "./Icons";

interface LanguageSwitcherProps {
  isDark: boolean;
}

export function LanguageSwitcher({ isDark }: LanguageSwitcherProps) {
  const { i18n } = useTranslation();
  const currentLang = i18n.language?.startsWith("zh") ? "zh" : "en";

  const toggleLanguage = () => {
    const nextLang = currentLang === "en" ? "zh" : "en";
    void i18n.changeLanguage(nextLang);
  };

  return (
    <button
      onClick={toggleLanguage}
      className={classNames(
        "flex items-center justify-center gap-1 px-2 h-9 rounded-xl transition-all min-h-[44px] glass-btn text-xs font-medium",
        isDark ? "text-slate-300 hover:text-white" : "text-gray-600 hover:text-gray-900"
      )}
      title={currentLang === "en" ? "Switch to Chinese" : "Switch to English"}
      aria-label={currentLang === "en" ? "Switch to Chinese" : "Switch to English"}
    >
      <GlobeIcon size={16} />
      <span className="hidden sm:inline">{currentLang === "en" ? "EN" : "\u4e2d"}</span>
    </button>
  );
}
