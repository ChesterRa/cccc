// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { apiJson } from "../services/api/base";
import { useConnectMentionGroups } from "./useConnectMentionGroups";

vi.mock("../services/api/base", () => ({ apiJson: vi.fn() }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
let result: ReturnType<typeof useConnectMentionGroups>;
function Probe({ group, open }: { group: string; open: boolean }) {
  result = useConnectMentionGroups(group, open);
  return null;
}

describe("composer catalog lifecycle", () => {
  beforeEach(() => {
    vi.mocked(apiJson).mockReset();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("loads on opening, discards late results from another Group, and retries when reopened", async () => {
    let resolveOld!: (value: { ok: true; result: unknown }) => void;
    const old = new Promise<{ ok: true; result: unknown }>((resolve) => {
      resolveOld = resolve;
    });
    vi.mocked(apiJson).mockImplementation(async (path) => {
      if (String(path).includes("/g_old/")) return old;
      return { ok: true, result: { instances: [], external_groups: [] } };
    });
    await act(async () => root.render(<Probe group="g_old" open={false} />));
    expect(apiJson).not.toHaveBeenCalled();
    await act(async () => root.render(<Probe group="g_old" open />));
    const signal = vi.mocked(apiJson).mock.calls[0][1]?.signal;
    expect(result.loading).toBe(true);
    await act(async () => root.render(<Probe group="g_new" open />));
    expect(signal?.aborted).toBe(true);
    expect(result.loading).toBe(false);
    await act(async () =>
      resolveOld({
        ok: true,
        result: { instances: [], external_groups: [], account_error: "old failure" },
      }),
    );
    expect(result.incomplete).toBe(false);
    await act(async () => root.render(<Probe group="g_new" open={false} />));
    expect(result.groups).toEqual([]);
    await act(async () => root.render(<Probe group="g_new" open />));
    expect(apiJson).toHaveBeenCalledTimes(3);
  });
});
