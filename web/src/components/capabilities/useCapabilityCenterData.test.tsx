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

function Harness({
  query,
  locale = "en",
  sourceId,
}: {
  query: string;
  locale?: string;
  sourceId?: string;
}) {
  data = useCapabilityCenterData({
    clientPagination: false,
    failedLoadMessage: "load failed",
    failedStateMessage: "state failed",
    groupId: "g-demo",
    isOpen: true,
    locale,
    pageIndex: 0,
    pageSize: 25,
    query,
    sourceId,
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

it("reads the list in the page's language and keeps the release's areas", async () => {
  const domains = [{ domain_id: "research", name: "搜索与调研" }];
  vi.mocked(api.fetchCapabilityOverview).mockResolvedValue({
    ok: true,
    result: { items: [], total_count: 0, has_more: false, domains },
  } as never);
  await act(async () => root.render(<Harness query="" locale="zh" sourceId="cccc_recommended" />));
  expect(vi.mocked(api.fetchCapabilityOverview)).toHaveBeenLastCalledWith(
    expect.objectContaining({ locale: "zh", sourceId: "cccc_recommended", groupId: "g-demo" }),
  );
  expect(data.domains).toEqual(domains);
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

it("keeps the matched query with the latest successful list, across late and failed reads", async () => {
  type Response = Awaited<ReturnType<typeof api.fetchCapabilityOverview>>;
  let resolveOlder!: (value: Response) => void;
  let resolveNewer!: (value: Response) => void;
  const older = new Promise<Response>((resolve) => {
    resolveOlder = resolve;
  });
  const newer = new Promise<Response>((resolve) => {
    resolveNewer = resolve;
  });
  vi.mocked(api.fetchCapabilityOverview).mockReturnValueOnce(older).mockReturnValueOnce(newer);

  await act(async () => root.render(<Harness query="older" />));
  await act(async () => root.render(<Harness query="data validation" />));
  expect(data.matchedQuery).toBe("");
  await act(async () => resolveNewer(overview as never));
  expect(data.matchedQuery).toBe("data validation");
  await act(async () => resolveOlder(overview as never));
  expect(data.matchedQuery).toBe("data validation");

  vi.mocked(api.fetchCapabilityOverview).mockResolvedValueOnce(failed as never);
  await act(async () => root.render(<Harness query="unavailable" />));
  expect(data.error).toBe("state read failed");
  expect(data.matchedQuery).toBe("data validation");
});
