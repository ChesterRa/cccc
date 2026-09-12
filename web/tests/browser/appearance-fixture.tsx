// Real appearance controls and both containing menus; all settings remain local to this fixture.
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "../../src/i18n";
import { AppSettingsMenu } from "../../src/components/layout/AppSettingsMenu";
import { MobileMenuSheet } from "../../src/components/layout/MobileMenuSheet";
import type { Theme, TextScale } from "../../src/types";
import "../../src/index.css";

await i18next.changeLanguage("en");
const noop = () => undefined;
const host = new URLSearchParams(location.search).get("host");
const calls: string[] = [];
Object.assign(window, { appearanceCalls: calls });
export function Fixture() {
  const [theme, setTheme] = useState<Theme>(
    new URLSearchParams(location.search).get("theme") === "dark" ? "dark" : "light",
  );
  const [textScale, setScale] = useState<TextScale>(100);
  const [open, setOpen] = useState(true);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme !== "dark");
    document.documentElement.style.fontSize = `${textScale}%`;
  }, [theme, textScale]);
  const appearance = {
    theme,
    textScale,
    onThemeChange: (value: Theme) => {
      calls.push(`theme:${value}`);
      setTheme(value);
    },
    onTextScaleChange: (value: TextScale) => {
      calls.push(`scale:${value}`);
      setScale(value);
    },
  };
  return host === "sheet" ? (
    <MobileMenuSheet
      {...appearance}
      isOpen={open}
      onClose={() => {
        calls.push("close");
        setOpen(false);
      }}
      selectedGroupId=""
      groupDoc={null}
      selectedGroupRunning={false}
      actors={[]}
      busy=""
      onOpenSearch={noop}
      onOpenContext={noop}
      onOpenSettings={noop}
      canAccessAccount
      onOpenAccount={noop}
      onStartGroup={noop}
      onStopGroup={noop}
      onSetGroupState={noop}
    />
  ) : (
    <div style={{ position: "absolute", right: 24, top: 24 }}>
      <AppSettingsMenu
        {...appearance}
        canAccessAccount
        canOpenSettings
        onOpenAccount={noop}
        onOpenSettings={noop}
      />
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
