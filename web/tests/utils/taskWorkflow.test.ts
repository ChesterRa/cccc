import { describe, expect, it } from "vite-plus/test";
import {
  TASK_TYPES,
  evaluateTaskWorkflow,
  getTaskDisplaySummary,
  getTaskDoneTransitionBlockers,
  getTaskTypeDefinition,
  recommendTaskType,
  resolveTaskType,
} from "../../src/utils/taskWorkflow";

describe("taskWorkflow", () => {
  it("ships the compact task-type roster", () => {
    expect(TASK_TYPES.map((item) => item.id)).toEqual(["free", "standard", "optimization"]);
  });

  it("exposes built-in type definitions without hidden seed behavior", () => {
    const standard = getTaskTypeDefinition("standard");
    expect(standard.requirements).toContain(
      "Capture the goal, success criteria, and required evidence.",
    );
    expect(standard.starterNotes).toContain("Goal:");
    expect(standard.starterChecklist).toContain("Define success criteria");

    const optimization = getTaskTypeDefinition("optimization");
    expect(optimization.requirements).toContain(
      "Capture the baseline, primary metric, and verifier boundary.",
    );
    expect(optimization.starterNotes).toContain("Baseline:");
    expect(optimization.starterNotes).toContain("Attempt Decision:");
    expect(optimization.starterChecklist).toContain("Record keep or discard");
  });

  it("defaults root work to standard and subtasks to free", () => {
    expect(recommendTaskType({ parentId: "" })).toBe("standard");
    expect(recommendTaskType({ parentId: "T001" })).toBe("free");
    expect(resolveTaskType({ parent_id: "" })).toBe("standard");
    expect(resolveTaskType({ parent_id: "T001" })).toBe("free");
  });

  it("prefers explicit persisted task types over structure defaults", () => {
    expect(resolveTaskType({ parent_id: "T001", task_type: "optimization", notes: "" })).toBe(
      "optimization",
    );

    expect(
      resolveTaskType({ parent_id: "T001", task_type: "standard", notes: "Baseline:\n- 120 ms" }),
    ).toBe("standard");
  });

  it("collapses legacy task-type aliases into the new taxonomy", () => {
    expect(resolveTaskType({ task_type: "root" })).toBe("standard");
    expect(resolveTaskType({ task_type: "reviewer" })).toBe("standard");
    expect(resolveTaskType({ task_type: "release" })).toBe("standard");
    expect(resolveTaskType({ parent_id: "T001", task_type: "lean" })).toBe("free");
  });

  it("does not infer specialist types from notes or checklist alone", () => {
    expect(
      resolveTaskType({
        notes: [
          "Goal:",
          "- Ship v0.4.7",
          "",
          "Success Criteria:",
          "- Smoke and version drift line up",
          "",
          "Required Evidence:",
          "- Smoke verification",
        ].join("\n"),
        checklist: [
          "[x] Update the release note",
          "[x] Check version and docs drift",
          "[ ] Run smoke verification",
        ].join("\n"),
      }),
    ).toBe("standard");

    expect(
      resolveTaskType({
        notes: "",
        checklist: [
          "[x] Capture the baseline",
          "[x] Name the primary metric",
          "[ ] Freeze the verifier boundary",
        ].join("\n"),
      }),
    ).toBe("standard");
  });

  it("treats free tasks as lightweight work with no contract gate", () => {
    const coverage = evaluateTaskWorkflow({
      parent_id: "T001",
      task_type: "free",
      status: "active",
      notes: "",
      checklist: "",
    });

    expect(coverage.taskTypeId).toBe("free");
    expect(coverage.isRoot).toBe(false);
    expect(coverage.needsContract).toBe(false);
    expect(coverage.missingSetup).toEqual([]);
  });

  it("applies standard contract coverage to explicit standard tasks", () => {
    const incomplete = evaluateTaskWorkflow({
      task_type: "standard",
      status: "active",
      assignee: "",
      notes: [
        "Goal:",
        "- Ship the web access panel refresh",
        "",
        "Success Criteria:",
        "- [define the observable finish line]",
      ].join("\n"),
    });

    expect(incomplete.needsContract).toBe(true);
    expect(incomplete.missingSetup).toEqual(["Success criteria", "Required evidence", "Owner"]);

    const complete = evaluateTaskWorkflow({
      task_type: "standard",
      status: "active",
      assignee: "foreman",
      notes: [
        "Goal:",
        "- Ship the web access panel refresh",
        "",
        "Success Criteria:",
        "- Users can switch local / private / public without ambiguity",
        "",
        "Required Evidence:",
        "- Verified in Linux and Windows smoke checks",
      ].join("\n"),
    });

    expect(complete.needsContract).toBe(false);
    expect(complete.goalSummary).toContain("Ship the web access panel refresh");
  });

  it("requires explicit closeout on done standard tasks", () => {
    const coverage = evaluateTaskWorkflow({
      task_type: "standard",
      assignee: "foreman",
      status: "done",
      outcome: "Web access panel refreshed and shipped.",
      notes: [
        "Goal:",
        "- Refresh the web access panel",
        "",
        "Success Criteria:",
        "- Users understand reachability and restart flow",
        "",
        "Required Evidence:",
        "- Manual verification on Linux and Windows",
        "",
        "Closeout Verdict:",
        "- DONE",
      ].join("\n"),
    });

    expect(coverage.needsContract).toBe(false);
    expect(coverage.needsCloseout).toBe(true);
    expect(coverage.missingCloseout).toEqual(["Verification summary"]);
  });

  it("treats optimization tasks as metric-sensitive work", () => {
    const coverage = evaluateTaskWorkflow({
      task_type: "optimization",
      assignee: "peer1",
      status: "done",
      outcome: "Attempt cut cold-start latency from 410 ms to 290 ms.",
      notes: [
        "Goal:",
        "- Reduce cold-start latency for the group context page",
        "",
        "Success Criteria:",
        "- Median latency drops without breaking cache freshness",
        "",
        "Required Evidence:",
        "- Bench output and smoke checks",
        "",
        "Baseline:",
        "- 410 ms median",
        "",
        "Primary Metric:",
        "- Median cold-start latency",
        "",
        "Verifier Boundary:",
        "- Same browser profile and same mock dataset",
        "",
        "Closeout Verdict:",
        "- DONE_WITH_CONCERNS",
        "",
        "Verification Summary:",
        "- Benchmark and manual smoke both passed",
      ].join("\n"),
    });

    expect(coverage.isOptimization).toBe(true);
    expect(coverage.needsContract).toBe(false);
    expect(coverage.needsCloseout).toBe(true);
    expect(coverage.missingCloseout).toEqual(["Attempt decision"]);
  });

  it("extracts optimization exploration residue without adding new task state", () => {
    const coverage = evaluateTaskWorkflow({
      task_type: "optimization",
      assignee: "peer1",
      status: "active",
      notes: [
        "Goal:",
        "- Reduce cold-start latency for the context page",
        "",
        "Success Criteria:",
        "- Median latency drops without stale cache regressions",
        "",
        "Required Evidence:",
        "- Bench output and smoke verification",
        "",
        "Baseline:",
        "- 410 ms median",
        "",
        "Primary Metric:",
        "- Median cold-start latency",
        "",
        "Verifier Boundary:",
        "- Same browser profile and same dataset",
        "",
        "Current Best:",
        "- Tightened parser cache invalidation and kept cache freshness stable",
        "",
        "Frontier Next:",
        "- Try narrower hydration deferral on the heaviest panel only",
        "",
        "Attempt Log:",
        "- [discard] widened preload scope | metric: 410 ms -> 405 ms | evidence: bench-a",
        "- [keep] tightened parser cache invalidation | metric: 410 ms -> 290 ms | evidence: bench-b",
      ].join("\n"),
    });

    expect(coverage.hasCurrentBest).toBe(true);
    expect(coverage.currentBestSummary).toContain("Tightened parser cache invalidation");
    expect(coverage.hasFrontierNext).toBe(true);
    expect(coverage.frontierNextSummary).toContain("narrower hydration deferral");
    expect(coverage.hasAttemptLog).toBe(true);
    expect(coverage.latestAttemptVerdict).toBe("keep");
    expect(coverage.latestAttemptSummary).toContain("tightened parser cache invalidation");
  });

  it("does not treat raw starter placeholders as completed workflow fields", () => {
    const optimization = getTaskTypeDefinition("optimization");
    const coverage = evaluateTaskWorkflow({
      task_type: "optimization",
      assignee: "",
      status: "active",
      notes: optimization.starterNotes,
      checklist: optimization.starterChecklist,
      outcome: "",
    });

    expect(coverage.needsContract).toBe(true);
    expect(coverage.hasGoal).toBe(false);
    expect(coverage.hasSuccessCriteria).toBe(false);
    expect(coverage.hasRequiredEvidence).toBe(false);
    expect(coverage.hasBaseline).toBe(false);
    expect(coverage.hasPrimaryMetric).toBe(false);
    expect(coverage.hasVerifierBoundary).toBe(false);
    expect(coverage.hasCurrentBest).toBe(false);
    expect(coverage.hasFrontierNext).toBe(false);
    expect(coverage.hasAttemptLog).toBe(false);
  });

  it("uses outcome, then current best, then goal as the display summary fallback chain", () => {
    expect(
      getTaskDisplaySummary({
        task_type: "standard",
        notes: "Goal:\n- Ship the 0.4.7 release cleanly",
        outcome: "",
      }),
    ).toContain("Ship the 0.4.7 release cleanly");

    expect(
      getTaskDisplaySummary({
        task_type: "standard",
        notes: "Goal:\n- Ship the 0.4.7 release cleanly",
        outcome: "Release shipped with smoke verification.",
      }),
    ).toBe("Release shipped with smoke verification.");

    expect(
      getTaskDisplaySummary({
        task_type: "optimization",
        notes: [
          "Goal:",
          "- Reduce cold-start latency",
          "",
          "Current Best:",
          "- Tightened parser cache invalidation and held cache freshness stable",
        ].join("\n"),
        outcome: "",
      }),
    ).toContain("Tightened parser cache invalidation");
  });

  it("blocks done transition only when closeout is incomplete for standard or optimization work", () => {
    expect(
      getTaskDoneTransitionBlockers({ taskType: "free", parentId: "T001", outcome: "", notes: "" }),
    ).toEqual([]);

    expect(
      getTaskDoneTransitionBlockers({
        taskType: "standard",
        assignee: "foreman",
        outcome: "Shipped the panel refresh.",
        notes: [
          "Goal:",
          "- Refresh the panel",
          "",
          "Success Criteria:",
          "- Users can switch reachability modes clearly",
          "",
          "Required Evidence:",
          "- Manual smoke checks",
          "",
          "Closeout Verdict:",
          "- DONE",
        ].join("\n"),
      }),
    ).toEqual(["Verification summary"]);

    expect(
      getTaskDoneTransitionBlockers({
        taskType: "optimization",
        assignee: "peer1",
        outcome: "Reduced cold-start time from 410 ms to 290 ms.",
        notes: [
          "Goal:",
          "- Reduce cold-start latency",
          "",
          "Success Criteria:",
          "- Median latency drops without regression",
          "",
          "Required Evidence:",
          "- Bench output and smoke checks",
          "",
          "Baseline:",
          "- 410 ms median",
          "",
          "Primary Metric:",
          "- Median cold-start latency",
          "",
          "Verifier Boundary:",
          "- Same browser profile",
          "",
          "Closeout Verdict:",
          "- DONE",
          "",
          "Verification Summary:",
          "- Bench output captured",
        ].join("\n"),
      }),
    ).toEqual(["Attempt decision"]);
  });
});
