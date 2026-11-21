import { describe, expect, it } from "vite-plus/test";
import settingsEn from "../../i18n/locales/en/settings.json";
import settingsJa from "../../i18n/locales/ja/settings.json";
import settingsZh from "../../i18n/locales/zh/settings.json";
import type {
  CapabilityMemberProjection,
  CapabilityOverviewItem,
  CapabilityVerification,
} from "../../types";
import {
  RECOMMENDATION_LEGEND,
  RECOMMENDATION_REASON_CODES,
  firstRecommendation,
  recommendationBinding,
  recommendationCanEnableForGroup,
  recommendationCount,
  recommendationDot,
  recommendationMemberDot,
  recommendationRows,
  recommendationSourceLabel,
  recommendationVerification,
  recommendationVerificationKey,
} from "./recommendationModel";

function member(
  availability: CapabilityMemberProjection["availability"],
  extra: Partial<CapabilityMemberProjection> = {},
): CapabilityMemberProjection {
  return {
    actor_id: "a",
    availability,
    enabled: false,
    blocked: false,
    reason_codes: [],
    enabled_scope: null,
    enabled_scopes: [],
    last_call: null,
    ...extra,
  };
}

function item(id: string, extra: Partial<CapabilityOverviewItem> = {}): CapabilityOverviewItem {
  return { capability_id: id, name: id.split(":").pop(), ...extra };
}

describe("recommendationDot", () => {
  it("takes the first rule that holds", () => {
    const compatible = member("compatible");
    expect(
      recommendationDot({ blocked_group: true, group_enabled: true, members: [compatible] }),
    ).toBe("blocked");
    expect(recommendationDot({ members: [] })).toBe("none");
    expect(recommendationDot({ group_enabled: true, members: [compatible] })).toBe("group");
    expect(
      recommendationDot({
        members: [compatible, member("compatible", { enabled_scopes: ["session"] })],
      }),
    ).toBe("member");
    expect(recommendationDot({ members: [compatible, member("needs_setup")] })).toBe("available");
    expect(recommendationDot({ members: [member("needs_setup"), member("unknown")] })).toBe(
      "setup",
    );
    expect(recommendationDot({ members: [member("unknown"), member("unsupported")] })).toBe(
      "unknown",
    );
    expect(recommendationDot({ members: [member("unsupported")] })).toBe("unsupported");
  });

  it("does not show a Group binding nobody can use as usable", () => {
    expect(recommendationDot({ group_enabled: true, members: [member("unsupported")] })).toBe(
      "unsupported",
    );
    expect(recommendationBinding({ group_enabled: true, members: [member("unsupported")] })).toBe(
      "group",
    );
  });

  it("does not count a blocked or unusable member's own binding", () => {
    const blocked = member("compatible", { blocked: true, enabled_scopes: ["actor"] });
    expect(recommendationDot({ members: [blocked] })).toBe("unsupported");
    const unusable = member("unknown", { enabled_scopes: ["actor"] });
    expect(recommendationDot({ members: [unusable] })).toBe("unknown");
    expect(recommendationBinding({ members: [unusable] })).toBe("member");
    expect(recommendationMemberDot({}, blocked)).toBe("blocked");
  });

  it("keeps a member whose token is not checked yet out of the ready states", () => {
    const unchecked = member("compatible", { activation_pending: true, enabled_scopes: ["actor"] });
    expect(recommendationDot({ group_enabled: true, members: [unchecked] })).toBe("pending");
    expect(recommendationDot({ members: [unchecked, member("compatible")] })).toBe("pending");
    expect(recommendationDot({ members: [unchecked, member("needs_setup")] })).toBe("pending");
    expect(
      recommendationDot({ group_enabled: true, members: [unchecked, member("compatible")] }),
    ).toBe("group");
    expect(recommendationMemberDot({ group_enabled: true }, unchecked)).toBe("pending");
    expect(recommendationBinding({ members: [unchecked] })).toBe("member");
    expect(recommendationCanEnableForGroup({ members: [unchecked] })).toBe(true);
  });

  it("offers Group enabling only when a member can or might use it", () => {
    expect(recommendationCanEnableForGroup({ members: [member("compatible")] })).toBe(true);
    expect(recommendationCanEnableForGroup({ members: [member("unknown")] })).toBe(true);
    expect(recommendationCanEnableForGroup({ members: [member("needs_setup")] })).toBe(false);
    expect(recommendationCanEnableForGroup({ members: [member("unsupported")] })).toBe(false);
    expect(recommendationCanEnableForGroup({ members: [] })).toBe(false);
    expect(
      recommendationCanEnableForGroup({ blocked_global: true, members: [member("compatible")] }),
    ).toBe(false);
  });
});

describe("recommendationVerification", () => {
  const observed = (
    level: CapabilityVerification["level"],
    outcome: CapabilityVerification["outcome"],
    observed_at: string,
  ): CapabilityVerification => ({ level, outcome, observed_at });

  it("shows the highest level, and the latest at that level", () => {
    const top = recommendationVerification([
      observed("install_checked", "passed", "2026-11-12T16:46:48Z"),
      observed("tool_call_checked", "failed", "2026-11-12T15:00:00Z"),
      observed("tool_call_checked", "passed", "2026-11-12T16:46:50Z"),
      observed("listed", "passed", "2026-11-13T00:00:00Z"),
    ]);
    expect(top?.observed_at).toBe("2026-11-12T16:46:50Z");
    expect(recommendationVerificationKey(top)).toBe("passed.tool_call_checked");
  });

  it("names a failed check and an empty record", () => {
    const top = () =>
      recommendationVerification([observed("tool_call_checked", "passed", "2026-11-12T16:00:00Z")]);
    const failed = recommendationVerification([
      observed("install_checked", "failed", "2026-11-12T16:00:00Z"),
    ]);
    expect(recommendationVerificationKey(failed)).toBe("failed.install_checked");
    // A text skill's install check compares its text, so it is never called installed.
    expect(recommendationVerificationKey(failed, "text_skill")).toBe("failed.text_checked");
    expect(recommendationVerificationKey(top(), "text_skill")).toBe("passed.tool_call_checked");
    expect(recommendationVerificationKey(recommendationVerification([]))).toBe("none");
    expect(
      recommendationVerification([
        { level: "guessed", outcome: "passed" } as unknown as CapabilityVerification,
      ]),
    ).toBeNull();
  });
});

describe("recommendationRows", () => {
  const domains = [
    { domain_id: "research", name: "Research" },
    { domain_id: "data", name: "Data" },
    { domain_id: "empty", name: "Empty" },
  ];
  const rows = recommendationRows(
    [
      item("mcp:recommended:zeta", {
        kind: "mcp_toolpack",
        recommendation: [{ domain_id: "research", role: "alternative", reason: "z" }],
      }),
      item("mcp:recommended:exa", {
        kind: "mcp_toolpack",
        recommendation: [{ domain_id: "research", role: "primary", reason: "search" }],
      }),
      item("mcp:recommended:alpha", {
        kind: "mcp_toolpack",
        recommendation: [{ domain_id: "research", role: "alternative", reason: "a" }],
      }),
      item("skill:recommended:validate-data", {
        kind: "skill",
        recommendation: [
          { domain_id: "data", role: "primary", reason: "check" },
          { domain_id: "research", role: "alternative", reason: "also" },
        ],
      }),
    ],
    domains,
  );

  it("keeps the release's domain order and puts the primary first", () => {
    expect(rows.map((row) => row.domain.domain_id)).toEqual(["research", "data", "empty"]);
    expect(rows[0].mcp.primary?.capability_id).toBe("mcp:recommended:exa");
    expect(rows[0].mcp.alternatives.map((value) => value.capability_id)).toEqual([
      "mcp:recommended:alpha",
      "mcp:recommended:zeta",
    ]);
    expect(rows[0].skill.primary).toBeNull();
    expect(rows[0].skill.alternatives.map((value) => value.capability_id)).toEqual([
      "skill:recommended:validate-data",
    ]);
    expect(rows[1].skill.primary?.capability_id).toBe("skill:recommended:validate-data");
    expect(rows[2].mcp.primary).toBeNull();
    expect(rows[2].skill.alternatives).toEqual([]);
  });

  it("counts each capability once and opens on the first primary", () => {
    expect(recommendationCount(rows)).toBe(4);
    expect(firstRecommendation(rows)).toBe("mcp:recommended:exa");
    expect(firstRecommendation([])).toBe("");
  });
});

it("shows a source address without its scheme", () => {
  expect(recommendationSourceLabel("https://github.com/exa-labs/exa-mcp-server/")).toBe(
    "github.com/exa-labs/exa-mcp-server",
  );
});

it("has every label it can show in all three languages", () => {
  for (const settings of [settingsEn, settingsJa, settingsZh]) {
    const recommended = settings.capabilityCenter.recommended as unknown as Record<
      string,
      Record<string, unknown>
    >;
    for (const dot of [...RECOMMENDATION_LEGEND, "pending", "uncertain"]) {
      expect(recommended.dot[dot], dot).toBeTruthy();
    }
    for (const source of ["actor", "profile"]) {
      expect(recommended.credential[source], source).toBeTruthy();
    }
    const update = settings.capabilityCenter.update as unknown as Record<
      string,
      Record<string, string>
    >;
    for (const reason of ["delivery_form_not_supported", "credential_route_not_supported"]) {
      expect(update.unavailable[reason], reason).toBeTruthy();
    }
    for (const state of ["configured", "ready", "unchanged"]) {
      expect(update.outcome[state], state).toBeTruthy();
    }
    for (const code of RECOMMENDATION_REASON_CODES) {
      expect(recommended.reason[code], code).toBeTruthy();
    }
    const verification = recommended.verification as Record<string, Record<string, string>>;
    for (const outcome of ["passed", "failed"]) {
      for (const level of [
        "listed",
        "install_checked",
        "text_checked",
        "tool_call_checked",
        "task_checked",
      ]) {
        expect(verification[outcome]?.[level], `${outcome}.${level}`).toBeTruthy();
      }
    }
    expect(recommended.verification.none).toBeTruthy();
  }
});
