// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PresentationPinModal } from "./PresentationPinModal";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
const listing = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", () => ({ fetchPresentationWorkspaceListing: listing }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  listing.mockResolvedValue({
    ok: true,
    result: {
      root_path: "/repo",
      path: "",
      parent: null,
      items: [{ name: "report.html", path: "report.html", is_dir: false }],
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.clearAllMocks();
});

const sourceRadio = (value: string) =>
  host.querySelector<HTMLInputElement>(`input[type="radio"][value="${value}"]`)!;
const urlInput = () => host.querySelector<HTMLInputElement>('input[type="url"]');

it("switches the form to the chosen source card and submits from that source", async () => {
  const onSubmitUrl = vi.fn();
  await act(async () =>
    root.render(
      <PresentationPinModal
        isOpen
        isDark={false}
        groupId="g"
        slot={{ slot_id: "slot-2", index: 2, card: null }}
        busy={false}
        onClose={vi.fn()}
        onSubmitUrl={onSubmitUrl}
        onSubmitWorkspace={vi.fn()}
        onSubmitFile={vi.fn()}
      />,
    ),
  );
  // A new pin starts from URL; the three sources are one radio group.
  expect(sourceRadio("url").checked).toBe(true);
  expect(sourceRadio("workspace").name).toBe(sourceRadio("upload").name);
  expect(urlInput()).not.toBeNull();

  await act(async () => sourceRadio("workspace").click());
  expect(sourceRadio("workspace").checked).toBe(true);
  expect(sourceRadio("url").checked).toBe(false);
  expect(listing).toHaveBeenCalledWith("g", "");
  expect(urlInput()).toBeNull();
  expect(host.textContent).toContain("report.html");

  await act(async () => sourceRadio("url").click());
  const input = urlInput()!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      input,
      "https://example.com/report",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const submit = [...host.querySelectorAll("button")].find(
    (button) => button.textContent === "presentationPinSubmit",
  )!;
  await act(async () => submit.click());
  expect(onSubmitUrl).toHaveBeenCalledWith(
    expect.objectContaining({ slotId: "slot-2", url: "https://example.com/report" }),
  );
});
