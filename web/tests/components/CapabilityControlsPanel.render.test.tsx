import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { CapabilityControlsPanel } from "../../src/components/capabilities/CapabilityControlsPanel";
import type { CapabilityOverviewItem } from "../../src/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

function row(overrides: Partial<CapabilityOverviewItem> = {}): CapabilityOverviewItem {
  return {
    capability_id: "skill:demo:review",
    kind: "skill",
    name: "review",
    source_id: "manual_import",
    ...overrides,
  };
}

describe("CapabilityControlsPanel rendering", () => {
  it("shows grouped skill controls without creating a Settings surface", () => {
    const html = renderToStaticMarkup(
      <CapabilityControlsPanel
        row={row()}
        enabled
        hidden={false}
        blocked={false}
        canShowSlashToggle
        removalAction="remove"
        busyKey=""
        groupId="g1"
        onToggleEnable={vi.fn()}
        onToggleSlash={vi.fn()}
        onToggleBlock={vi.fn()}
        onRemove={vi.fn()}
      />,
    );

    expect(html).toContain("capabilityCenter.controls.title");
    expect(html).toContain("capabilityCenter.controls.skillHint");
    expect(html).toContain("capabilityCenter.hideFromSlashCommands");
    expect(html).toContain("capabilityCenter.remove.label.remove");
    expect(html).not.toContain("Settings");
  });

  it("uses MCP-specific copy and omits slash visibility when unavailable", () => {
    const html = renderToStaticMarkup(
      <CapabilityControlsPanel
        row={row({ capability_id: "mcp:demo/server", kind: "mcp_toolpack" })}
        enabled={false}
        hidden={false}
        blocked
        canShowSlashToggle={false}
        removalAction="uninstall"
        busyKey=""
        groupId="g1"
        onToggleEnable={vi.fn()}
        onToggleSlash={vi.fn()}
        onToggleBlock={vi.fn()}
        onRemove={vi.fn()}
      />,
    );

    expect(html).toContain("capabilityCenter.controls.mcpHint");
    expect(html).toContain("capabilityCenter.remove.label.uninstall");
    expect(html).not.toContain("capabilityCenter.hideFromSlashCommands");
  });
});
