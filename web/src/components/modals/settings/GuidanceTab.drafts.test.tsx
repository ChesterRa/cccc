// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { GuidanceTab } from "./GuidanceTab";
import * as api from "../../../services/api";
const prompt = (kind: string, content: string) => ({
  kind,
  filename: `${kind}.md`,
  path: "fixture",
  source: "home",
  content,
});
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  Trans: () => null,
}));
vi.mock("../../../services/api", () => ({
  fetchGroupPrompts: vi.fn(async () => ({
    ok: true,
    result: {
      preamble: prompt("preamble", "Original preamble"),
      help: prompt("help", "Original help"),
    },
  })),
  fetchActors: async () => ({ ok: true, result: { actors: [] } }),
  updateGroupPrompt: vi.fn(async (_group: string, kind: string, content: string) => ({
    ok: true,
    result: prompt(kind, content),
  })),
}));
let root: ReturnType<typeof createRoot> | null = null;
let host: HTMLDivElement;
afterEach(async () => {
  await act(async () => root?.unmount());
  host.remove();
  vi.clearAllMocks();
});
it("saving preamble preserves the unsaved help draft instead of reloading both editors", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<GuidanceTab isDark={false} groupId="g" />));
  const edit = async (index: number, value: string) => {
    const input = host.querySelectorAll("textarea")[index];
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        value,
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  expect(host.querySelectorAll("textarea").length).toBe(2);
  await edit(0, "Updated preamble");
  await edit(1, "Unsaved help");
  const save = [...host.querySelectorAll("button")].find((b) => b.textContent === "common:save")!;
  await act(async () => save.click());
  expect(api.updateGroupPrompt).toHaveBeenCalledWith("g", "preamble", "Updated preamble");
  expect(host.querySelectorAll("textarea")[1].value).toBe("Unsaved help");
  expect(api.fetchGroupPrompts).toHaveBeenCalledTimes(1);
});
