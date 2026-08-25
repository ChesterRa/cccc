import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { AppHeader } from "./AppHeader";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

const noop = () => undefined;

describe("AppHeader account entry", () => {
  it("keeps an accessible icon-only account button in the desktop utility rail", () => {
    const html = renderToStaticMarkup(
      <AppHeader
        isDark={false}
        theme="light"
        textScale={100}
        onThemeChange={noop}
        onTextScaleChange={noop}
        selectedGroupId=""
        groupDoc={null}
        selectedGroupRunning={false}
        selectedGroupRuntimeStatus={null}
        actors={[]}
        sseStatus="connected"
        busy=""
        onOpenSidebar={noop}
        onOpenSearch={noop}
        onOpenContext={noop}
        onStartGroup={noop}
        onStopGroup={noop}
        onSetGroupState={noop}
        onOpenSettings={noop}
        canAccessAccount
        onOpenAccount={noop}
        onOpenMobileMenu={noop}
      />,
    );

    const accountButton = html.match(
      /<button(?=[^>]*aria-label="account")(?=[^>]*title="account")[^>]*>([\s\S]*?)<\/button>/,
    );
    expect(accountButton).not.toBeNull();
    expect(accountButton?.[1]).not.toContain("account");
  });
});
