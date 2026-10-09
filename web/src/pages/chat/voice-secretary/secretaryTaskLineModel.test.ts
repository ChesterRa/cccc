import { describe, expect, it } from "vite-plus/test";
import type { SecretaryTaskSummary } from "../../../types";
import { secretaryTaskNeedsAttention, summarizeSecretaryTasks } from "./secretaryTaskLineModel";

const base: SecretaryTaskSummary = {
  task_id: "t",
  target: {
    group_id: "A",
    scope_key: "s",
    kind: "document",
    document_path: "d.md",
    request_id: "r",
  },
  phase: "done",
  cleanup_confirmed: true,
  created_at: "",
  updated_at: "",
  source_count: 1,
  preview: "",
  diagnostic: "",
  projection_error: "",
  previous_task_id: "",
  superseded_by: "",
  candidate_available: false,
};
const task = (patch: Partial<SecretaryTaskSummary>) => ({ ...base, ...patch });

describe("secretary task line summary", () => {
  it("reports running work first and counts queued work and unprocessed sources", () => {
    expect(
      summarizeSecretaryTasks(
        [task({ phase: "running" }), task({ phase: "queued" }), task({ phase: "failed" })],
        { unprocessed: 2 },
      ),
    ).toEqual({ kind: "running", count: 2, pending: 2 });
  });

  it("surfaces unresolved problems when nothing is running", () => {
    expect(
      summarizeSecretaryTasks(
        [task({ phase: "needs_user" }), task({ phase: "done", projection_error: "x" })],
        { unprocessed: 0 },
      ),
    ).toEqual({ kind: "attention", count: 2 });
  });

  it("does not treat superseded or cancelled tasks as needing attention", () => {
    expect(secretaryTaskNeedsAttention(task({ phase: "failed", superseded_by: "next" }))).toBe(
      false,
    );
    expect(secretaryTaskNeedsAttention(task({ phase: "cancelled" }))).toBe(false);
    expect(summarizeSecretaryTasks([task({ phase: "cancelled" })], { unprocessed: 0 })).toEqual({
      kind: "recent",
      count: 1,
    });
  });

  it("is idle without tasks", () => {
    expect(summarizeSecretaryTasks([], { unprocessed: 0 })).toEqual({ kind: "idle" });
  });
});
