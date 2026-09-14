// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
const showError = vi.hoisted(() => vi.fn());
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../stores/useUIStore", () => ({ useUIStore: { getState: () => ({ showError }) } }));
import { AuthenticatedDownloadLink } from "./AuthenticatedDownloadLink";

describe("embedded authenticated downloads", () => {
  let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    showError.mockClear();
    const proof = {
      frame_id: "one",
      target_instance_id: "B",
      target_device_id: "b",
      parent_origin: "https://a.test",
      expires_at: "2099-01-01T00:00:00Z",
      signature: "server-verified",
    };
    window.history.replaceState(null, "", `/ui/connect/?proof=${btoa(JSON.stringify(proof))}`);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
    window.history.replaceState(null, "", "/ui/");
  });
  const click = async () => {
    await act(async () =>
      root.render(
        <AuthenticatedDownloadLink href="/api/v1/groups/g/blobs/file" download="report.txt">
          Report
        </AuthenticatedDownloadLink>,
      ),
    );
    await act(async () =>
      host
        .querySelector("a")
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })),
    );
  };
  it("uses the target cookie, creates a browser download and releases its Blob URL", async () => {
    const fetcher = vi.fn(async () => new Response("report"));
    vi.stubGlobal("fetch", fetcher);
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:http://localhost/report");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const saved: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(
      function (this: HTMLAnchorElement) {
        saved.push(this.download);
      },
    );
    await click();
    expect(fetcher).toHaveBeenCalledWith(
      expect.any(URL),
      expect.objectContaining({ credentials: "same-origin", redirect: "error" }),
    );
    expect(create).toHaveBeenCalledOnce();
    expect(saved).toEqual(["report.txt"]);
    expect(host.querySelector("a")?.getAttribute("aria-busy")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(30000));
    expect(revoke).toHaveBeenCalledWith("blob:http://localhost/report");
  });
  it("reports an authorization failure without saving the error response as a file", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("denied", { status: 403 })),
    );
    const create = vi.spyOn(URL, "createObjectURL");
    await click();
    expect(create).not.toHaveBeenCalled();
    expect(showError).toHaveBeenCalledWith("connect.downloadFailed");
    expect(host.querySelector("a")?.getAttribute("aria-busy")).toBeNull();
  });
  it("aborts the target read when its workbench leaves", async () => {
    let signal: AbortSignal | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn((_url, init: RequestInit) => {
        signal = init.signal as AbortSignal;
        return new Promise((_resolve, reject) =>
          signal?.addEventListener("abort", () =>
            reject(new DOMException("Aborted", "AbortError")),
          ),
        );
      }),
    );
    await click();
    expect(signal?.aborted).toBe(false);
    await act(async () => root.render(null));
    expect(signal?.aborted).toBe(true);
    expect(showError).not.toHaveBeenCalled();
  });
});
