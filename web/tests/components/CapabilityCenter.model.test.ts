import { describe, expect, it } from "vitest";

import {
  capabilityCenterDisplayName,
  capabilityCenterFilterItemsForSystemVisibility,
  capabilityCenterRemovalAction,
  capabilityCenterFrameClass,
  capabilityCenterRootClass,
  capabilityCenterSourceRemovalAction,
  capabilityCenterSourcesGridClass,
  capabilityCenterSectionTypeFilter,
  capabilityCenterPageRange,
  capabilityCenterPaginationMode,
  capabilityCenterType,
  filterCapabilityCenterItems,
  mergeCapabilityCenterStickyItems,
  normalizeCapabilityCenterPagination,
  summarizeCapabilityCenter,
} from "../../src/components/capabilities/capabilityCenterModel";
import type { CapabilityOverviewItem, CapabilityStateResult } from "../../src/types";

function row(overrides: Partial<CapabilityOverviewItem> & { capability_id: string }): CapabilityOverviewItem {
  return {
    capability_id: overrides.capability_id,
    kind: overrides.kind || "skill",
    name: overrides.name,
    description_short: overrides.description_short,
    source_id: overrides.source_id || "manual_import",
    policy_level: overrides.policy_level || "actionable",
    qualification_status: overrides.qualification_status,
    blocked_global: overrides.blocked_global,
    enable_supported: overrides.enable_supported,
    readiness_preview: overrides.readiness_preview,
    tags: overrides.tags,
    tool_names: overrides.tool_names,
  };
}

const state: CapabilityStateResult = {
  group_id: "g1",
  actor_id: "user",
  enabled: [{ capability_id: "skill:team:review" }],
  enabled_capabilities: ["mcp:context7", "skill:team:hidden"],
  actor_hidden_capabilities: ["skill:team:hidden"],
};

describe("CapabilityCenter model", () => {
  it("normalizes skills, MCP toolpacks, and packs into first-class center types", () => {
    expect(capabilityCenterType(row({ capability_id: "skill:team:review", kind: "skill" }))).toBe("skill");
    expect(capabilityCenterType(row({ capability_id: "mcp:context7", kind: "mcp_toolpack" }))).toBe("mcp");
    expect(capabilityCenterType(row({ capability_id: "pack:default", kind: "pack" }))).toBe("pack");
  });

  it("uses the capability id tail as a stable display fallback", () => {
    expect(capabilityCenterDisplayName(row({ capability_id: "skill:team:requesting-code-review" }))).toBe("requesting-code-review");
    expect(capabilityCenterDisplayName(row({ capability_id: "skill:team:review", name: "Reviewer" }))).toBe("Reviewer");
  });

  it("filters dense rows by type, query, enabled state, slash visibility, and health", () => {
    const items = [
      row({ capability_id: "skill:team:review", name: "Review", tags: ["code"] }),
      row({ capability_id: "skill:team:hidden", name: "Hidden skill" }),
      row({ capability_id: "skill:team:library-only", name: "Library only skill" }),
      row({ capability_id: "mcp:context7", kind: "mcp_toolpack", tool_names: ["resolve-library-id"] }),
      row({ capability_id: "pack:blocked", kind: "pack", blocked_global: true }),
      row({ capability_id: "skill:team:needs-env", readiness_preview: { preview_status: "missing_env" } }),
    ];

    expect(filterCapabilityCenterItems(items, { typeFilter: "mcp", state }).map((item) => item.capability_id)).toEqual(["mcp:context7"]);
    expect(filterCapabilityCenterItems(items, { query: "resolve-library", state }).map((item) => item.capability_id)).toEqual(["mcp:context7"]);
    expect(filterCapabilityCenterItems(items, { stateFilter: "enabled", state }).map((item) => item.capability_id)).toEqual([
      "skill:team:review",
      "skill:team:hidden",
      "mcp:context7",
    ]);
    expect(filterCapabilityCenterItems(items, { stateFilter: "slash_visible", state }).map((item) => item.capability_id)).toEqual([
      "skill:team:review",
    ]);
    expect(filterCapabilityCenterItems(items, { stateFilter: "slash_hidden", state }).map((item) => item.capability_id)).toEqual([
      "skill:team:hidden",
    ]);
    expect(filterCapabilityCenterItems(items, { stateFilter: "blocked", state }).map((item) => item.capability_id)).toEqual([
      "pack:blocked",
    ]);
    expect(filterCapabilityCenterItems(items, { stateFilter: "needs_setup", state }).map((item) => item.capability_id)).toEqual([
      "skill:team:needs-env",
    ]);
  });

  it("summarizes the global library without tying the count to card UI", () => {
    const stats = summarizeCapabilityCenter([
      row({ capability_id: "skill:team:review", kind: "skill", source_id: "manual_import" }),
      row({ capability_id: "skill:team:hidden", kind: "skill", source_id: "manual_import" }),
      row({ capability_id: "mcp:context7", kind: "mcp_toolpack", source_id: "mcp_registry_official" }),
      row({ capability_id: "pack:blocked", kind: "pack", source_id: "cccc_builtin", blocked_global: true }),
    ], state);

    expect(stats).toMatchObject({
      total: 4,
      skills: 2,
      mcp: 1,
      packs: 1,
      enabled: 3,
      slashHidden: 1,
      blocked: 1,
      sources: 3,
    });
  });

  it("normalizes pagination and reports the visible range", () => {
    expect(normalizeCapabilityCenterPagination({ pageIndex: -2, pageSize: 0, totalCount: 161 })).toEqual({
      pageIndex: 0,
      pageSize: 80,
      totalPages: 3,
      offset: 0,
    });
    expect(normalizeCapabilityCenterPagination({ pageIndex: 9, pageSize: 40, totalCount: 161 })).toEqual({
      pageIndex: 4,
      pageSize: 40,
      totalPages: 5,
      offset: 160,
    });
    expect(capabilityCenterPageRange({ pageIndex: 1, pageSize: 40, itemCount: 40, totalCount: 161 })).toEqual({
      from: 41,
      to: 80,
      total: 161,
    });
    expect(capabilityCenterPageRange({ pageIndex: 4, pageSize: 40, itemCount: 1, totalCount: 161 })).toEqual({
      from: 161,
      to: 161,
      total: 161,
    });
  });

  it("uses client pagination for state filters that cannot be represented by overview totals", () => {
    expect(capabilityCenterPaginationMode({ section: "skill", stateFilter: "enabled" })).toBe("client");
    expect(capabilityCenterPaginationMode({ section: "mcp", stateFilter: "enabled" })).toBe("client");
    expect(capabilityCenterPaginationMode({ section: "skill", stateFilter: "slash_hidden" })).toBe("client");
    expect(capabilityCenterPaginationMode({ section: "skill", stateFilter: "needs_setup" })).toBe("client");
    expect(capabilityCenterPaginationMode({ section: "skill", stateFilter: "blocked" })).toBe("server");
    expect(capabilityCenterPaginationMode({ section: "skill", stateFilter: "blocked" })).toBe("server");
    expect(capabilityCenterPaginationMode({ section: "mcp", stateFilter: "all" })).toBe("server");
  });

  it("keeps transiently changed rows in a filtered view without duplicating matches", () => {
    const visible = row({ capability_id: "skill:team:review", name: "Review" });
    const justHidden = row({ capability_id: "skill:team:hidden", name: "Hidden skill" });

    expect(mergeCapabilityCenterStickyItems([visible], [visible, justHidden]).map((item) => item.capability_id)).toEqual([
      "skill:team:review",
      "skill:team:hidden",
    ]);
  });

  it("maps primary sidebar sections to their overview type filters", () => {
    expect(capabilityCenterSectionTypeFilter("skill")).toBe("skill");
    expect(capabilityCenterSectionTypeFilter("mcp")).toBe("mcp");
    expect(capabilityCenterSectionTypeFilter("sources")).toBe("all");
  });

  it("protects inactive built-ins and labels row actions by capability type", () => {
    expect(capabilityCenterRemovalAction(row({
      capability_id: "skill:cccc:install",
      kind: "skill",
      source_id: "cccc_builtin",
    }))).toBe("none");
    expect(capabilityCenterRemovalAction(row({
      capability_id: "pack:diagnostics",
      kind: "pack",
      source_id: "cccc_builtin",
    }), { enabled: true })).toBe("disable");
    expect(capabilityCenterRemovalAction(row({
      capability_id: "skill:agent_self_proposed:triage",
      kind: "skill",
      source_id: "agent_self_proposed",
    }))).toBe("delete");
    expect(capabilityCenterRemovalAction(row({
      capability_id: "skill:github:obra:brainstorming",
      kind: "skill",
      source_id: "github_skills_curated",
    }))).toBe("remove");
    expect(capabilityCenterRemovalAction(row({
      capability_id: "mcp:example/server",
      kind: "mcp_toolpack",
      source_id: "mcp_registry_official",
    }))).toBe("uninstall");
  });

  it("only allows deleting user-owned capability sources", () => {
    expect(capabilityCenterSourceRemovalAction({ source_id: "manual_import", enabled: true, record_count: 3 })).toBe("delete");
    expect(capabilityCenterSourceRemovalAction({ source_id: "github_import", enabled: true, record_count: 2 })).toBe("delete");
    expect(capabilityCenterSourceRemovalAction({ source_id: "url_import", enabled: true, record_count: 1 })).toBe("delete");
    expect(capabilityCenterSourceRemovalAction({ source_id: "local_import", enabled: true, record_count: 1 })).toBe("delete");
    expect(capabilityCenterSourceRemovalAction({ source_id: "agent_self_proposed", enabled: true, record_count: 1 })).toBe("delete");
    expect(capabilityCenterSourceRemovalAction({ source_id: "github_skills_curated", enabled: true, record_count: 49 })).toBe("none");
    expect(capabilityCenterSourceRemovalAction({ source_id: "mcp_registry_official", enabled: true, record_count: 1450 })).toBe("none");
    expect(capabilityCenterSourceRemovalAction({ source_id: "cccc_builtin", enabled: true, record_count: 34 })).toBe("none");
  });

  it("reserves a stable actions column for source rows with and without delete", () => {
    const gridClass = capabilityCenterSourcesGridClass();

    expect(gridClass).toContain("_76px]");
    expect(gridClass).toContain("md:grid-cols");
  });

  it("keeps the page shell viewport-bound so only the center content scrolls", () => {
    expect(capabilityCenterRootClass("page")).toContain("h-dvh");
    expect(capabilityCenterRootClass("page")).toContain("overflow-hidden");
    expect(capabilityCenterFrameClass("page")).toContain("h-dvh");
    expect(capabilityCenterFrameClass("page")).not.toContain("min-h-dvh");
  });

  it("hides readonly builtin system rows by default but preserves search discoverability", () => {
    const items = [
      row({ capability_id: "pack:diagnostics", kind: "pack", source_id: "cccc_builtin", name: "Terminal Debug" }),
      row({ capability_id: "mcp:github/chrome-devtools-mcp", kind: "mcp_toolpack", source_id: "mcp_registry_official", name: "chrome devtools mcp" }),
      row({ capability_id: "pack:enabled-builtin", kind: "pack", source_id: "cccc_builtin", name: "Enabled Builtin" }),
    ];

    expect(
      capabilityCenterFilterItemsForSystemVisibility(items, {
        showSystem: false,
        query: "",
        state: {
          ...state,
          enabled: [{ capability_id: "pack:enabled-builtin" }],
        },
      }).map((item) => item.capability_id),
    ).toEqual(["mcp:github/chrome-devtools-mcp", "pack:enabled-builtin"]);

    expect(
      capabilityCenterFilterItemsForSystemVisibility(items, {
        showSystem: false,
        query: "terminal",
        state,
      }).map((item) => item.capability_id),
    ).toEqual(["pack:diagnostics"]);

    expect(
      capabilityCenterFilterItemsForSystemVisibility(items, {
        showSystem: true,
        query: "",
        state,
      }).map((item) => item.capability_id),
    ).toEqual(["pack:diagnostics", "mcp:github/chrome-devtools-mcp", "pack:enabled-builtin"]);
  });
});
