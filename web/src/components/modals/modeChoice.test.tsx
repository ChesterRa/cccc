// @vitest-environment happy-dom
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import { RuntimeConfigurationModePicker } from "./RuntimeProfileControls";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("mode choice buttons", () => {
  it("expose which choice is selected, for assistive technology and styling", () => {
    const host = document.createElement("div");
    host.innerHTML = renderToStaticMarkup(
      <RuntimeConfigurationModePicker value="profile" onChange={() => {}} />,
    );
    const pressed = [...host.querySelectorAll("button[data-mode-choice]")].map((button) => [
      button.textContent,
      button.getAttribute("aria-pressed"),
    ]);
    expect(pressed).toEqual([
      ["customAgent", "false"],
      ["fromActorProfile", "true"],
    ]);
  });
});
