import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { HeadlessRawTrace } from "./HeadlessRawTrace";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ i18n: { language: "en" } }) }));

describe("HeadlessRawTrace lifecycle status", () => {
  it.each([
    ["headless.turn.started", "active", true],
    ["headless.turn.cancelled", "inactive", false],
    ["headless.control.queued", "attention", false],
    ["headless.control.stalled", "attention", false],
    ["headless.turn.completed", "neutral", false],
  ] as const)("renders %s without a misleading running label", (type, dot, running) => {
    const html = renderToStaticMarkup(
      <HeadlessRawTrace
        events={[{ type, actor_id: "coder", data: { turn_id: "turn-1" } }]}
        emptyLabel="empty"
        isDark={false}
      />,
    );

    expect(html).toContain(`data-status-dot="${dot}"`);
    expect(html.includes(">running<")).toBe(running);
  });
});
