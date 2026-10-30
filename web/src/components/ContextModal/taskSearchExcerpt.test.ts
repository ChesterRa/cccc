import { expect, it } from "vite-plus/test";
import type { Task } from "../../types";
import { taskSearchExcerpt } from "./taskSearchExcerpt";

const task = (fields: Partial<Task>): Task => ({ id: "T7", title: "Board polish", ...fields });
// What a card shows: its title and id, plus whatever a case adds.
const shown = (...more: string[]) => ["T7", "Board polish", ...more];

it("says nothing when the card already shows the match", () => {
  expect(taskSearchExcerpt(task({ notes: "polish later" }), "POLISH", shown())).toBeNull();
  expect(taskSearchExcerpt(task({ notes: "ask codex-1" }), "t7", shown())).toBeNull();
  expect(taskSearchExcerpt(task({ notes: "codex-1" }), "codex", shown("codex-1"))).toBeNull();
  expect(taskSearchExcerpt(task({ outcome: "Ship the fix" }), "ship", shown("Ship"))).toBeNull();
  expect(taskSearchExcerpt(task({ notes: "anything" }), "   ", shown())).toBeNull();
  expect(taskSearchExcerpt(task({ notes: "anything" }), "missing", shown())).toBeNull();
});

it("quotes the matched field around the match on one line", () => {
  const notes = `Context:\n\n${"earlier words ".repeat(5)}the Flaky test\tfails\n${"later ".repeat(20)}`;
  const found = taskSearchExcerpt(task({ notes }), "flaky test", shown());
  expect(found?.field).toBe("notes");
  expect(found?.match).toBe("Flaky test");
  expect(found?.before.startsWith("…")).toBe(true);
  expect(found?.before.endsWith("the ")).toBe(true);
  expect(found?.after.startsWith(" fails")).toBe(true);
  expect(found?.after.endsWith("…")).toBe(true);
  expect(`${found?.before}${found?.match}${found?.after}`).not.toMatch(/[\n\t]/);

  const short = taskSearchExcerpt(task({ outcome: "Deploy after review" }), "review", shown());
  expect(short).toEqual({ field: "outcome", before: "Deploy after ", match: "review", after: "" });
  // A Done card hides its handoff, so the quote explains the match there.
  expect(taskSearchExcerpt(task({ handoff_to: "reviewer" }), "view", shown())?.field).toBe(
    "handoff_to",
  );
});

it("maps the match back to the original text when casing lengthens a character", () => {
  // "İ" lowercases to two units, which would shift every later offset.
  const found = taskSearchExcerpt(task({ notes: "İİİ İzmir rollout DONE" }), "done", shown());
  expect(found?.match).toBe("DONE");
  expect(found?.before).toBe("İİİ İzmir rollout ");
  expect(taskSearchExcerpt(task({ notes: "Check İzmir first" }), "i̇zmir", shown())?.match).toBe(
    "İzmir",
  );
  // With İ forcing the mapped path, a final sigma still cases as the server's
  // whole-string lowercase does: ΟΣ at a word end is ος, not οσ.
  expect(taskSearchExcerpt(task({ notes: "İΟΣ" }), "ος", shown())?.match).toBe("ΟΣ");
  expect(taskSearchExcerpt(task({ notes: "İΟΣ" }), "οσ", shown())).toBeNull();
});

it("never splits a character or emoji at the edge of the quote", () => {
  const family = "👨‍👩‍👧";
  const notes = `${family.repeat(30)} needle ${"😀".repeat(80)}`;
  const found = taskSearchExcerpt(task({ notes }), "needle", shown());
  expect(found?.match).toBe("needle");
  // Each emoji counts double, which keeps the match within the quote's two lines.
  expect(found?.before).toBe(`…${family.repeat(9)} `);
  expect(found?.after).toBe(` ${"😀".repeat(59)}…`);
});

// Speed is measured separately (2M characters: 10–19 ms in Node 24); a wall
// clock limit here would fail on a busy CI runner.
it("finds and maps matches in very long notes, also when casing lengthens a character", () => {
  for (const notes of [
    `${"x".repeat(2_000_000)} late match`,
    // One İ sends the search down the path that maps offsets back.
    `İ${"x".repeat(2_000_000)} late match`,
    `${"x".repeat(2_000_000)}İ late match`,
  ]) {
    const found = taskSearchExcerpt(task({ notes }), "late", shown());
    expect(found?.match).toBe("late");
    expect(found?.after).toBe(" match");
  }
  // A match right after a chunk edge, with an İ before it, maps exactly.
  const edge = `İ${"y".repeat(4094)}ZZ tail`;
  expect(taskSearchExcerpt(task({ notes: edge }), "zz", shown())?.match).toBe("ZZ");
});

it("starts the lead-in at a whole word and quotes a title the card clips", () => {
  const notes = "We discussed the rollback plan and then the flaky cache test again";
  expect(taskSearchExcerpt(task({ notes }), "cache", shown())?.before).toBe("…and then the flaky ");
  const title = "A long title that runs past the two lines a card shows, about zebra crossings";
  const found = taskSearchExcerpt(task({ title }), "zebra", ["T7", title.slice(0, 30)]);
  expect(found?.field).toBe("title");
  expect(found?.match).toBe("zebra");
});
