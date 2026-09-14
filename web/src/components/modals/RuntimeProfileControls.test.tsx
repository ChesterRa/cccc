import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { OpenCodeManagedModelHint } from "./RuntimeProfileControls";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("OpenCode model selection hint", () => {
  it("marks the hint icon decorative and keeps the hint out of the alert role", () => {
    const html = renderToStaticMarkup(<OpenCodeManagedModelHint runtime="opencode" />);
    expect(html).toContain("opencodeManagedModelHint");
    expect(html).toContain("<svg");
    expect(html).toContain('aria-hidden="true"');
    expect(html).not.toContain('role="alert"');
  });

  it("normalizes the selected runtime", () => {
    expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime=" OpenCode " />)).toContain(
      "opencodeManagedModelHint",
    );
  });

  it("also explains model synchronization for Kilo", () => {
    expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime="kilo" />)).toContain(
      "opencodeManagedModelHint",
    );
  });

  it("does not warn for other runtimes or an empty selection", () => {
    for (const runtime of ["codex", "claude", "grok", "", null, undefined]) {
      expect(renderToStaticMarkup(<OpenCodeManagedModelHint runtime={runtime} />)).toBe("");
    }
  });
});
