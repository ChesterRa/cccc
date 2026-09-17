// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { usePresentationAsset } from "./usePresentationAsset";

let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
let current: ReturnType<typeof usePresentationAsset>;
function Probe({
  href,
  identity,
  kind,
}: {
  href: string;
  identity: string;
  kind: "image" | "markdown" | null;
}) {
  current = usePresentationAsset(href, identity, kind);
  return <div>{current.content}</div>;
}
async function render(
  href = "/asset?1",
  identity = "first",
  kind: "image" | "markdown" | null = "markdown",
) {
  await act(async () => root.render(<Probe href={href} identity={identity} kind={kind} />));
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("first version")),
  );
  let sequence = 0;
  vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:asset-${++sequence}`);
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  vi.spyOn(HTMLImageElement.prototype, "decode").mockResolvedValue(undefined);
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it.each(["markdown", "image"] as const)(
  "keeps %s during slow refresh and transient failures, then recovers",
  async (kind) => {
    await render("/asset?1", "first", kind);
    const original = current.content;
    let respond!: (response: Response) => void;
    vi.mocked(fetch).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          respond = resolve;
        }),
    );
    await render("/asset?2", "first", kind);
    expect(current.refreshing.current).toBe(true);
    expect(current.content).toBe(original);
    await act(async () => respond(new Response("unavailable", { status: 503 })));
    expect(current.content).toBe(original);
    expect(current.stale).toBe(true);
    expect(current.refreshing.current).toBe(false);
    vi.mocked(fetch).mockRejectedValueOnce(new TypeError("Network unavailable"));
    await render("/asset?3", "first", kind);
    expect(current.content).toBe(original);
    expect(current.stale).toBe(true);
    vi.mocked(fetch).mockResolvedValueOnce(new Response("next version"));
    await render("/asset?4", "first", kind);
    expect(current.content).not.toBe(original);
    expect(current.stale).toBe(false);
    expect(current.error).toBe("");
    if (kind === "image") expect(URL.revokeObjectURL).toHaveBeenCalledWith(original);
  },
);

it.each([401, 403, 404, 409, 410])(
  "clears old content after definitive HTTP %s",
  async (status) => {
    await render("/asset?1", "first", "image");
    const original = current.content;
    vi.mocked(fetch).mockResolvedValueOnce(new Response("unavailable", { status }));
    await render("/asset?2", "first", "image");
    expect(current.content).toBeNull();
    expect(current.stale).toBe(false);
    expect(current.error).toBe(`HTTP ${status}`);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(original);
  },
);

it("never shows another resource while loading and ignores its late response", async () => {
  await render();
  let respond!: (response: Response) => void;
  vi.mocked(fetch).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        respond = resolve;
      }),
  );
  await render("/asset?2");
  const signal = vi.mocked(fetch).mock.calls.at(-1)![1]!.signal!;
  vi.mocked(fetch).mockRejectedValueOnce(new TypeError("Network unavailable"));
  await render("/other", "other");
  expect(signal.aborted).toBe(true);
  expect(current.content).toBeNull();
  expect(current.stale).toBe(false);
  await act(async () => respond(new Response("late old content")));
  expect(current.content).toBeNull();
  expect(current.error).toBe("Network unavailable");
});

it("waits for image decoding and releases failed and replaced blobs", async () => {
  await render("/asset?1", "first", "image");
  const original = current.content;
  let decode!: () => void;
  vi.mocked(HTMLImageElement.prototype.decode).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        decode = resolve;
      }),
  );
  await render("/asset?2", "first", "image");
  expect(current.content).toBe(original);
  await act(async () => decode());
  const replacement = current.content;
  expect(replacement).not.toBe(original);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith(original);
  vi.mocked(HTMLImageElement.prototype.decode).mockRejectedValueOnce(new Error("Invalid image"));
  await render("/asset?3", "first", "image");
  expect(current.content).toBe(replacement);
  expect(current.stale).toBe(true);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:asset-3");
  await render("", "first", null);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith(replacement);
});

it("does not confuse an empty loaded document with a first-load failure", async () => {
  vi.mocked(fetch).mockResolvedValueOnce(new Response("unavailable", { status: 503 }));
  await render();
  expect(current.content).toBeNull();
  expect(current.stale).toBe(false);
  vi.mocked(fetch).mockResolvedValueOnce(new Response(""));
  await render("/asset?2");
  expect(current.content).toBe("");
  vi.mocked(fetch).mockResolvedValueOnce(new Response("unavailable", { status: 503 }));
  await render("/asset?3");
  expect(current.content).toBe("");
  expect(current.stale).toBe(true);
});
