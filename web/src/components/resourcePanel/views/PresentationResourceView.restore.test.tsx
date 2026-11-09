// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PresentationResourceView } from "./PresentationResourceView";

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), viewer: vi.fn() }));
vi.mock("../../../services/api", () => ({ fetchMessageWindow: mocks.fetch }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../presentation/PresentationViewerModal", () => ({
  PresentationViewerSplitPanel: (props: unknown) => {
    mocks.viewer(props);
    return <div data-restored-viewer />;
  },
}));

let host: HTMLDivElement;
let root: Root;
const base = {
  groupId: "g",
  slotId: "slot-2",
  presentation: null,
  isDark: false,
  visible: true,
  focusEventId: "event",
  referenceKey: "event:1",
  handlers: {},
};
const quote = (slot_id = "slot-2") => ({
  kind: "presentation_ref",
  slot_id,
  locator: { scroll_top: 100 },
});
const response = (refs: unknown[]) => ({
  ok: true,
  result: { events: [{ id: "event", data: { refs } }] },
});
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  mocks.fetch.mockReset();
  mocks.viewer.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("fetches the exact message and uses the filtered Presentation reference index", async () => {
  const selected = quote();
  let finish!: (value: unknown) => void;
  mocks.fetch.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => root.render(<PresentationResourceView {...base} />));
  expect(host.textContent).toContain("common:loading");
  expect(mocks.viewer).not.toHaveBeenCalled();
  expect(mocks.fetch).toHaveBeenCalledWith("g", "event", { before: 0, after: 0 });
  await act(async () =>
    finish(response([{ kind: "task_ref", task_id: "task" }, quote("slot-1"), selected])),
  );
  expect(mocks.viewer).toHaveBeenLastCalledWith(expect.objectContaining({ focusRef: selected }));
});

it.each([response([quote(), quote("slot-3")]), response([])])(
  "does not mount a live slot when the saved quote cannot be recovered",
  async (result) => {
    mocks.fetch.mockResolvedValue(result);
    await act(async () => root.render(<PresentationResourceView {...base} />));
    expect(host.textContent).toContain("resourcePanel.referenceUnavailable");
    expect(mocks.viewer).not.toHaveBeenCalled();
  },
);

it.each([false, true])("offers a retry for a failed request (rejected=%s)", async (rejected) => {
  if (rejected) mocks.fetch.mockRejectedValueOnce(new Error("connection lost"));
  else
    mocks.fetch.mockResolvedValueOnce({
      ok: false,
      error: { code: "permission_denied", message: "Access denied" },
    });
  mocks.fetch.mockResolvedValueOnce(response([quote(), quote()]));
  await act(async () => root.render(<PresentationResourceView {...base} />));
  expect(host.textContent).toContain(
    rejected ? "resourcePanel.referenceLoadFailed" : "Access denied",
  );
  expect(host.textContent).not.toContain("resourcePanel.referenceUnavailable");
  expect(mocks.viewer).not.toHaveBeenCalled();
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(mocks.fetch).toHaveBeenCalledTimes(2);
  expect(mocks.viewer).toHaveBeenCalledWith(expect.objectContaining({ focusRef: quote() }));
});

it("waits until an unresolved quote is shown before fetching it", async () => {
  mocks.fetch.mockResolvedValue(response([quote(), quote()]));
  await act(async () => root.render(<PresentationResourceView {...base} visible={false} />));
  expect(mocks.fetch).not.toHaveBeenCalled();
  await act(async () => root.render(<PresentationResourceView {...base} />));
  expect(mocks.fetch).toHaveBeenCalledTimes(1);
  expect(host.querySelector("[data-restored-viewer]")).not.toBeNull();
});

it("ignores a late quote response after the resource changes", async () => {
  let finish!: (value: unknown) => void;
  mocks.fetch.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => root.render(<PresentationResourceView {...base} />));
  await act(async () =>
    root.render(<PresentationResourceView {...base} referenceKey="live" focusEventId={null} />),
  );
  await act(async () => finish(response([quote(), quote()])));
  expect(mocks.viewer).toHaveBeenLastCalledWith(
    expect.objectContaining({ focusRef: null, focusEventId: null }),
  );
});
