import { describe, expect, it } from "vitest";

import {
  capabilityRegistryActionKind,
  capabilityOverviewLoadKey,
  capabilityRecommendationEntries,
  canEditSkillRecord,
  canManageSkillAssignments,
  canManageSlashCommandVisibility,
  isCapabilityHiddenFromSlashCommands,
  nextSlashCommandHiddenCapabilities,
} from "../../src/components/modals/settings/capabilityManagementModel";

describe("CapabilitiesTab model", () => {
  it("allows assignment management for any imported skill capability", () => {
    expect(canManageSkillAssignments({
      capability_id: "skill:github:obra-superpowers:requesting-code-review",
      kind: "skill",
      source_id: "manual_import",
    })).toBe(true);
    expect(canManageSkillAssignments({
      capability_id: "skill:agent_self_proposed:triage",
      kind: "skill",
      source_id: "agent_self_proposed",
    })).toBe(true);
  });

  it("does not show skill assignment management for packs or MCP toolpacks", () => {
    expect(canManageSkillAssignments({
      capability_id: "mcp:context7",
      kind: "mcp_toolpack",
      source_id: "manual_import",
    })).toBe(false);
    expect(canManageSkillAssignments({
      capability_id: "pack:group-runtime",
      kind: "pack",
      source_id: "cccc_builtin",
    })).toBe(false);
  });

  it("shows first-level slash command visibility controls only for skill capabilities", () => {
    expect(canManageSlashCommandVisibility({
      capability_id: "skill:github:obra-superpowers:requesting-code-review",
      kind: "skill",
    })).toBe(true);
    expect(canManageSlashCommandVisibility({
      capability_id: "mcp:context7",
      kind: "mcp_toolpack",
    })).toBe(false);
    expect(canManageSlashCommandVisibility({
      capability_id: "pack:group-runtime",
      kind: "pack",
    })).toBe(false);
  });

  it("only allows editing local self-proposed skill records", () => {
    expect(canEditSkillRecord({
      capability_id: "skill:agent_self_proposed:triage",
      kind: "skill",
      source_id: "agent_self_proposed",
    })).toBe(true);
    expect(canEditSkillRecord({
      capability_id: "skill:github:obra-superpowers:requesting-code-review",
      kind: "skill",
      source_id: "manual_import",
    })).toBe(false);
  });

  it("routes imported skills to assignment management instead of block-only actions", () => {
    expect(capabilityRegistryActionKind({
      capability_id: "skill:agent_self_proposed:triage",
      kind: "skill",
      source_id: "agent_self_proposed",
    })).toBe("edit-skill");
    expect(capabilityRegistryActionKind({
      capability_id: "skill:github:obra-superpowers:requesting-code-review",
      kind: "skill",
      source_id: "manual_import",
    })).toBe("manage-skill-assignments");
    expect(capabilityRegistryActionKind({
      capability_id: "mcp:context7",
      kind: "mcp_toolpack",
      source_id: "manual_import",
    })).toBe("block");
  });

  it("keeps the overview load key focused on query and filters", () => {
    const input = {
      isActive: true,
      debouncedQuery: " install ",
      registryKind: "skill",
      registryPageSize: 40,
      registryPolicy: "all",
      registrySource: "all",
      selfEvolvingSurface: false,
    };

    expect(capabilityOverviewLoadKey(input)).toBe(capabilityOverviewLoadKey({ ...input }));
    expect(capabilityOverviewLoadKey({ ...input, debouncedQuery: "review" })).not.toBe(capabilityOverviewLoadKey(input));
  });

  it("updates slash command hidden skills without dropping unrelated preferences", () => {
    const hidden = ["skill:cccc:install", "skill:team:writer"];

    expect(isCapabilityHiddenFromSlashCommands("skill:team:writer", hidden)).toBe(true);
    expect(nextSlashCommandHiddenCapabilities(hidden, "skill:team:writer", true)).toEqual(["skill:cccc:install"]);
    expect(nextSlashCommandHiddenCapabilities(hidden, "skill:team:reviewer", false)).toEqual([
      "skill:cccc:install",
      "skill:team:writer",
      "skill:team:reviewer",
    ]);
    expect(nextSlashCommandHiddenCapabilities(hidden, "skill:team:writer", false)).toEqual(hidden);
  });

  it("builds compact recommendation entries for skill management rows", () => {
    expect(capabilityRecommendationEntries({
      capability_id: "skill:team:reviewer",
      kind: "skill",
      use_when: ["Review code", "Ignore second line"],
      evidence_kind: "pytest",
      gotchas: ["Needs repo context"],
      avoid_when: [],
    })).toEqual([
      { key: "use_when", value: "Review code" },
      { key: "verify_with", value: "pytest" },
      { key: "gotcha", value: "Needs repo context" },
    ]);
  });
});
