// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SearchModal } from "./SearchModal";
import { apiJson } from "../services/api";
import type { Actor } from "../types";
vi.mock("../services/api", () => ({ apiJson: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.query ? `${key}: ${values.query}` : key,
  }),
}));
let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
const response = (text: string, hasMore = false) => ({
  ok: true as const,
  result: {
    events: [
      { id: text, kind: "chat.message", ts: "2026-09-17T00:00:00Z", by: "worker", data: { text } },
    ],
    has_more: hasMore,
    count: 1,
  },
});
const render = (groupId = "g1", isOpen = true) =>
  act(async () =>
    root.render(
      <SearchModal
        isOpen={isOpen}
        groupId={groupId}
        groupTitle="Release workspace"
        actors={[{ id: "worker", title: "Review lead" } as Actor]}
        isDark={false}
        onClose={() => {}}
        onReply={() => {}}
      />,
    ),
  );
async function query(value: string) {
  await act(async () => {
    const input = host.querySelector("input")!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const submit = () =>
  act(async () => {
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
const click = (text: string) =>
  act(async () => {
    [...host.querySelectorAll("button")].find((button) => button.textContent === text)!.click();
  });
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.mocked(apiJson).mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
it("starts with guidance, a readable Group and Actor names, without querying", async () => {
  await render();
  expect(host.textContent).toContain("searchStart");
  expect(host.textContent).not.toContain("noResults");
  expect(host.textContent).toContain("Release workspace");
  expect(host.querySelector("option[value=worker]")!.textContent).toBe("Review lead");
  expect(apiJson).not.toHaveBeenCalled();
});
it("distinguishes an empty result from a network failure", async () => {
  await render();
  vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { events: [], has_more: false } });
  await submit();
  expect(host.textContent).toContain("noResults");
  vi.mocked(apiJson).mockRejectedValueOnce(new Error("offline"));
  await submit();
  expect(host.querySelector('[role="alert"]')!.textContent).toBe("searchFailed");
  expect(host.textContent).not.toContain("noResults");
});
it("uses the submitted query for highlighting, filters and pagination while a new draft is typed", async () => {
  await render();
  await query("alpha");
  vi.mocked(apiJson).mockResolvedValue(response("alpha found", true));
  await submit();
  await query("beta");
  expect(host.querySelector("mark")!.textContent).toBe("alpha");
  await click("loadOlderResults");
  expect(String(vi.mocked(apiJson).mock.lastCall?.[0])).toContain("q=alpha");
  expect(String(vi.mocked(apiJson).mock.lastCall?.[0])).toContain("before=alpha");
  await click("kindNotify");
  expect(String(vi.mocked(apiJson).mock.lastCall?.[0])).toContain("q=alpha&kind=notify");
  expect(String(vi.mocked(apiJson).mock.lastCall?.[0])).not.toContain("before=");
});
it("ignores an older response after a filter changes", async () => {
  let resolve!: (value: ReturnType<typeof response>) => void;
  vi.mocked(apiJson).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await render();
  await query("alpha");
  await submit();
  vi.mocked(apiJson).mockResolvedValueOnce(response("new result"));
  await click("kindChat");
  await act(async () => resolve(response("old result")));
  expect(host.textContent).toContain("new result");
  expect(host.textContent).not.toContain("old result");
});
it.each(["close", "group"])(
  "ignores pending results after %s and resets the new search",
  async (action) => {
    let resolve!: (value: ReturnType<typeof response>) => void;
    vi.mocked(apiJson).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    await render();
    await submit();
    await render(action === "group" ? "g2" : "g1", action !== "close");
    await act(async () => resolve(response("previous Group")));
    if (action === "close") await render();
    expect(host.textContent).not.toContain("previous Group");
    expect(host.textContent).toContain("searchStart");
  },
);
