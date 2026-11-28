// @vitest-environment happy-dom
import { setUiStylePrefs } from "../../utils/uiStyle";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PendingSummaryContext } from "../../features/pendingReplies/context";
import {
  createPendingReplyStore,
  PendingReplyStoreContext,
} from "../../features/pendingReplies/store";
import { PendingReplySlot } from "./PendingReplySlot";
import { requestEvent as event } from "./PendingReplySlot.fixtures";

// Network boundary: withdrawing a reply request, and answering one.
const api = vi.hoisted(() => ({ cancel: vi.fn(), answer: vi.fn() }));
vi.mock("../../services/api/context", () => ({ cancelReplyRequest: api.cancel }));
vi.mock("../../services/api/pendingReplies", () => ({ answerPendingReply: api.answer }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement;
let root: Root;
const refresh = vi.fn();

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.clearAllMocks();
  act(() => setUiStylePrefs({ style: "preview" }));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "lcd" });
});

async function render() {
  const store = createPendingReplyStore();
  await act(async () =>
    root.render(
      <PendingSummaryContext.Provider value={{ summaries: {}, refresh }}>
        <PendingReplyStoreContext.Provider value={store}>
          <PendingReplySlot event={event} groupId="g1" onUseComposer={() => {}} />
        </PendingReplyStoreContext.Provider>
      </PendingSummaryContext.Provider>,
    ),
  );
  return store;
}
const dismiss = () => host.querySelector<HTMLButtonElement>("[data-ask-dismiss]");

it("withdraws a question in one click, then shows it withdrawn", async () => {
  api.cancel.mockResolvedValue({ ok: true, result: {} });
  await render();
  expect(dismiss()?.textContent).toBe("pendingReply.dismiss");

  await act(async () => dismiss()!.click());
  expect(api.cancel).toHaveBeenCalledTimes(1);
  expect(api.cancel).toHaveBeenCalledWith("g1", "r1");
  expect(refresh).toHaveBeenCalledTimes(1);
  // The card is closed as withdrawn and offers no more actions.
  expect(host.textContent).toContain("pendingReply.withdrawn");
  expect(dismiss()).toBeNull();
  expect(host.querySelector('input[type="radio"]:not(:disabled)')).toBeNull();
});

it("keeps the question open and says so when it cannot be withdrawn", async () => {
  api.cancel.mockResolvedValue({ ok: false, error: { code: "permission_denied" } });
  await render();
  await act(async () => dismiss()!.click());
  expect(host.textContent).toContain("pendingReply.dismissFailed");
  expect(host.textContent).not.toContain("pendingReply.withdrawn");
  expect(dismiss()).not.toBeNull();
  expect(refresh).not.toHaveBeenCalled();
});

it("offers no withdrawing once an answer is on its way", async () => {
  api.answer.mockReturnValue(new Promise(() => {}));
  await render();
  await act(async () => host.querySelector<HTMLInputElement>('input[type="radio"]')!.click());
  expect(dismiss()).not.toBeNull();
  await act(async () =>
    host
      .querySelector<HTMLFormElement>("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(api.answer).toHaveBeenCalled();
  expect(dismiss()).toBeNull();
});

it("offers no withdrawing on the card in the pixel style", async () => {
  act(() => setUiStylePrefs({ style: "lcd" }));
  await render();
  expect(host.querySelector("[data-ask-dismiss]")).toBeNull();
  // The question itself is still shown and answerable.
  expect(host.querySelector("[data-ask-send]")).not.toBeNull();
});
