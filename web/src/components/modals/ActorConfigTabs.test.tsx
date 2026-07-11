import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ActorConfigTabs } from "./ActorConfigTabs";
import { nextActorConfigTabId } from "./actorConfigTabsModel";

const tabs = [
  { id: "environment", label: "Environment variables", panel: <div>Environment panel</div> },
  { id: "capabilities", label: "Capabilities", panel: <div>Capabilities panel</div> },
  { id: "profile", label: "Runtime profile tools", panel: <div>Profile panel</div> },
];

describe("ActorConfigTabs", () => {
  it("renders one selected tab and only its associated panel", () => {
    const markup = renderToStaticMarkup(
      <ActorConfigTabs
        ariaLabel="Advanced settings"
        tabs={tabs}
        activeId="capabilities"
        onChange={() => undefined}
      />
    );

    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('aria-label="Advanced settings"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain("Capabilities panel");
    expect(markup).not.toContain("Environment panel");
    expect(markup).not.toContain("Profile panel");
  });

  it.each([
    ["environment", "ArrowRight", "capabilities"],
    ["environment", "ArrowLeft", "profile"],
    ["profile", "ArrowRight", "environment"],
    ["capabilities", "Home", "environment"],
    ["environment", "End", "profile"],
    ["environment", "Enter", "environment"],
  ])("moves from %s with %s to %s", (activeId, key, expected) => {
    expect(nextActorConfigTabId(tabs.map((tab) => tab.id), activeId, key)).toBe(expected);
  });
});
