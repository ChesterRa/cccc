import { describe, expect, it } from "vitest";
import {
  BUILTIN_ROLE_PRESETS,
  getRolePresetApplyState,
  getRolePresetById,
} from "../../src/utils/rolePresets";

describe("rolePresets", () => {
  it("ships the first-wave built-in preset roster", () => {
    expect(BUILTIN_ROLE_PRESETS.map((item) => item.id)).toEqual([
      "coordinator",
      "planner",
      "implementer",
      "reviewer",
      "debugger",
      "explorer",
      "researcher",
    ]);
  });

  it("ships structured content for every preset", () => {
    for (const preset of BUILTIN_ROLE_PRESETS) {
      expect(preset.name.trim()).not.toBe("");
      expect(preset.summary.trim()).not.toBe("");
      expect(preset.useWhen.trim()).not.toBe("");
      expect(preset.content).toContain("### Mission");
      expect(preset.content).toContain("### Hard Rules");
    }
  });

  it("keeps the high-leverage gstack transplant kernels in the most relevant presets", () => {
    const planner = getRolePresetById("planner")?.content || "";
    expect(planner).toContain("what existing code, workflow, or pattern already solves part of this?");
    expect(planner).toContain("minimum set of changes");
    expect(planner).toContain("complete version");
    expect(planner).toContain("post-review version, not the first draft");

    const implementer = getRolePresetById("implementer")?.content || "";
    expect(implementer).toContain("hard self-review before handoff");

    const reviewer = getRolePresetById("reviewer")?.content || "";
    expect(reviewer).toContain("Read the full diff before forming findings.");
    expect(reviewer).toContain("docs, diagrams, or adjacent tests went stale");

    const debuggerPreset = getRolePresetById("debugger")?.content || "";
    expect(debuggerPreset).toContain("Confirm the leading hypothesis");
    expect(debuggerPreset).toContain("regression test");

    const coordinator = getRolePresetById("coordinator")?.content || "";
    expect(coordinator).toContain("not governance authority");
    expect(coordinator).toContain("Do not behave like a second foreman");

    const explorer = getRolePresetById("explorer")?.content || "";
    expect(explorer).toContain("existing code already solves it partially");
  });

  it("looks up presets by id", () => {
    expect(getRolePresetById("implementer")?.name).toBe("Implementer");
    expect(getRolePresetById("")).toBeNull();
    expect(getRolePresetById("missing")).toBeNull();
  });

  it("computes draft replacement behavior conservatively", () => {
    const preset = "preset body";
    expect(getRolePresetApplyState("", preset)).toBe("apply");
    expect(getRolePresetApplyState("  ", preset)).toBe("apply");
    expect(getRolePresetApplyState("preset body", preset)).toBe("no_change");
    expect(getRolePresetApplyState("existing draft", preset)).toBe("confirm_replace");
    expect(getRolePresetApplyState("existing draft", "")).toBe("no_change");
  });
});
