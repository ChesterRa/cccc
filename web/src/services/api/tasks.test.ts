import { expect, it, vi } from "vite-plus/test";
import * as base from "./base";
import { fetchTaskPages } from "./tasks";

vi.mock("./base", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./base")>()),
  apiJson: vi.fn(),
}));

it("distinguishes a zero result count from a missing or malformed count", async () => {
  vi.mocked(base.apiJson).mockResolvedValueOnce({
    ok: true,
    result: {
      pages: { planned: { total_count: 0 }, active: {}, done: { total_count: "3" } },
      tasks_version: "tasksv:1",
    },
  });
  const response = await fetchTaskPages("g-test", { statuses: ["planned", "active", "done"] });
  expect(response.ok).toBe(true);
  if (!response.ok) throw new Error("test fixture");
  expect(response.result.pages.planned?.totalCountKnown).toBe(true);
  expect(response.result.pages.active?.totalCountKnown).toBe(false);
  expect(response.result.pages.done?.totalCountKnown).toBe(false);
});
