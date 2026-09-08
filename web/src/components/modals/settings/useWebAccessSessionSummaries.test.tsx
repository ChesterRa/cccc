import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import type { WebAccessSession } from "../../../types";
import { useWebAccessSessionSummaries } from "./useWebAccessSessionSummaries";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function Summary({ session }: { session: WebAccessSession }) {
  const { currentBrowserSummary } = useWebAccessSessionSummaries(1, session.login_active, session);
  return (
    <p>
      {currentBrowserSummary.label} {currentBrowserSummary.detail}
    </p>
  );
}

describe("Web Access session summary", () => {
  it.each([false, true])(
    "identifies local administrator access with token login active=%s",
    (login_active) => {
      const html = renderToStaticMarkup(
        <Summary
          session={{
            login_active,
            current_browser_signed_in: true,
            principal_kind: "local",
            is_admin: true,
          }}
        />,
      );
      expect(html).toContain("webAccess.currentBrowserLocal");
      expect(html).not.toContain("webAccess.currentBrowserOpen");
    },
  );

  it("keeps remote sign-in and unauthenticated browsers separate from local access", () => {
    for (const signedIn of [false, true]) {
      const html = renderToStaticMarkup(
        <Summary
          session={{
            login_active: true,
            current_browser_signed_in: signedIn,
            principal_kind: signedIn ? "token" : "anonymous",
          }}
        />,
      );
      expect(html).not.toContain("webAccess.currentBrowserLocal");
      expect(html).toContain(
        signedIn ? "webAccess.currentBrowserSignedIn" : "webAccess.currentBrowserNotSignedIn",
      );
    }
  });
});
