// Browser fixture: real panel, service and layout; only the HTTP response is stubbed.
// Rust history_page tests separately cover raw ANSI -> retained frames.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import actors from "../../src/i18n/locales/en/actors.json";
import common from "../../src/i18n/locales/en/common.json";
import { TerminalHistoryPanel } from "../../src/components/agentTerminal/TerminalHistoryPanel";
import "../../src/index.css";

await i18next.use(initReactI18next).init({ lng: "en", resources: { en: { actors, common } } });
const requests: string[] = [];
Object.assign(window, { historyRequests: requests });
window.fetch = async (input) => {
  const url = String(input);
  if (!url.startsWith("/api/v1/groups/history-fixture/terminal/history?")) {
    throw new Error(`Unexpected fixture request: ${url}`);
  }
  requests.push(url);
  const older = new URL(url, location.origin).searchParams.has("before");
  return Response.json({
    ok: true,
    result: {
      text: older ? "old frame\n\nnew frame" : "new frame",
      start_cursor: older ? 0 : 15,
      end_cursor: 40,
      has_more: !older,
      cursor_expired: false,
      hint: "",
    },
  });
};
export function Fixture() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open history</button>
      {open && (
        <TerminalHistoryPanel
          groupId="history-fixture"
          actorId="actor"
          actorTitle={"Long actor name ".repeat(12)}
          isDark={false}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
