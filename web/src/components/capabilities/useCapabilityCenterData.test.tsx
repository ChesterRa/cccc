// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import { useCapabilityCenterData } from "./useCapabilityCenterData";

vi.mock("../../services/api", () => ({
  fetchCapabilityOverview: vi.fn(),
  fetchGroupCapabilityState: vi.fn(),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const overview = { ok: true, result: { items: [], total_count: 0, has_more: false } };
const readable = { ok: true, result: { enabled_capabilities: ["pack:diagnostics"] } };
const failed = { ok: false, error: { code: "unavailable", message: "state read failed" } };

let data: ReturnType<typeof useCapabilityCenterData>;

function Harness({ query }: { query: string }) {
  data = useCapabilityCenterData({
    clientPagination: false,
    failedLoadMessage: "load failed",
    failedStateMessage: "state failed",
    groupId: "g-demo",
    isOpen: true,
    pageIndex: 0,
    pageSize: 25,
    query,
    stateFilter: "all",
    typeFilter: "all",
  });
  return null;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  vi.mocked(api.fetchCapabilityOverview).mockResolvedValue(overview as never);
  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue(readable as never);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<Harness query="" />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
});

it("keeps a failed state read on screen until a later state read succeeds", async () => {
  expect(data.error).toBe("");

  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue(failed as never);
  await act(async () => void (await data.refresh()));
  expect(data.error).toBe("state read failed");
  expect(data.state?.enabled_capabilities).toEqual(["pack:diagnostics"]);

  // Filtering reloads only the list, which must not clear the stale-state warning.
  await act(async () => root.render(<Harness query="self" />));
  expect(vi.mocked(api.fetchCapabilityOverview)).toHaveBeenCalledTimes(3);
  expect(data.error).toBe("state read failed");

  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue(readable as never);
  await act(async () => void (await data.refresh()));
  expect(data.error).toBe("");
});

it("offers state reloads only through its checked read, never a raw state setter", async () => {
  // Actions that change state (the "/" visibility toggle) reload through loadState, so an
  // unusable or failed read is reported the same way as a refresh.
  expect("setState" in data).toBe(false);
  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue({ ok: true, result: {} } as never);
  await act(async () => void (await data.loadState()));
  expect(data.error).toBe("state failed");
  expect(data.state?.enabled_capabilities).toEqual(["pack:diagnostics"]);

  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue(failed as never);
  await act(async () => void (await data.loadState()));
  expect(data.error).toBe("state read failed");
});

it("does not show a state without its enabled list as an empty one", async () => {
  vi.mocked(api.fetchGroupCapabilityState).mockResolvedValue({ ok: true, result: {} } as never);
  let next: unknown;
  await act(async () => {
    next = await data.refresh();
  });
  expect(next).toBeNull();
  expect(data.error).toBe("state failed");
  expect(data.state?.enabled_capabilities).toEqual(["pack:diagnostics"]);
});
