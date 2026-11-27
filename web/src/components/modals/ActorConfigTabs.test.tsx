import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vite-plus/test";

import { ActorConfigTabs } from "./ActorConfigTabs";
import { nextActorConfigTabId } from "./actorConfigTabsModel";

const tabs = [
  { id: "environment", label: "Environment variables", panel: <div>Environment panel</div> },
  { id: "capabilities", label: "Capabilities", panel: <div>Capabilities panel</div> },
  { id: "profile", label: "Runtime profile tools", panel: <div>Profile panel</div> },
];

describe("ActorConfigTabs", () => {
  it("keeps every panel mounted and hides inactive panels", () => {
    const markup = renderToStaticMarkup(
      <ActorConfigTabs
        ariaLabel="Advanced settings"
        tabs={tabs}
        activeId="capabilities"
        onChange={() => undefined}
      />,
    );

    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('aria-label="Advanced settings"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain("Capabilities panel");
    expect(markup).toContain("Environment panel");
    expect(markup).toContain("Profile panel");
    expect(markup.match(/role="tabpanel"/g)).toHaveLength(3);
    expect(markup.match(/role="tabpanel"[^>]*hidden=""/g)).toHaveLength(2);
  });

  it("sets an aside tab apart from its siblings and names what needs attention behind it", () => {
    const markup = renderToStaticMarkup(
      <ActorConfigTabs
        ariaLabel="Add Agent"
        tabs={[
          ...tabs,
          {
            id: "advanced",
            label: "Advanced",
            aside: true,
            attention: "2 environment variables",
            panel: <div>Advanced panel</div>,
          },
        ]}
        activeId="environment"
        onChange={() => undefined}
      />,
    );
    const tabMarkup = markup.match(/<button[^>]*role="tab".*?<\/button>/g)!;

    expect(tabMarkup).toHaveLength(4);
    expect(tabMarkup.map((tab) => tab.includes("data-aside"))).toEqual([false, false, false, true]);
    // The attention text is part of the tab's accessible name, not only a coloured dot.
    expect(tabMarkup[3].replace(/<svg.*?<\/svg>|<[^>]+>/g, "")).toBe(
      "Advanced2 environment variables",
    );
    expect(tabMarkup[0].replace(/<[^>]+>/g, "")).toBe("Environment variables");
  });

  it.each([
    ["environment", "ArrowRight", "capabilities"],
    ["environment", "ArrowLeft", "profile"],
    ["profile", "ArrowRight", "environment"],
    ["capabilities", "Home", "environment"],
    ["environment", "End", "profile"],
    ["environment", "Enter", "environment"],
  ])("moves from %s with %s to %s", (activeId, key, expected) => {
    expect(
      nextActorConfigTabId(
        tabs.map((tab) => tab.id),
        activeId,
        key,
      ),
    ).toBe(expected);
  });
});
