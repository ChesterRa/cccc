// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useDeepLink } from "./useDeepLink";
import type { GroupMeta } from "../types";
import { apiJson } from "../services/api/base";
import {
  finishWorkspaceNavigation,
  requestWorkspaceNavigation,
  setWorkspaceDirty,
  useWorkspaceNavigation,
} from "../stores/workspaceNavigation";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const translate = vi.hoisted(
  () => (key: string, values?: { groupId?: string }) =>
    key === "notifications.groupNotFound" ? `Group not found: ${values?.groupId}` : key,
);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));
vi.mock("../services/api/base", () => ({ apiJson: vi.fn() }));
let root: ReturnType<typeof createRoot>;
const serviceWorkerDescriptor = Object.getOwnPropertyDescriptor(navigator, "serviceWorker");
afterEach(async () => {
  await act(async () => root.unmount());
  vi.mocked(apiJson).mockReset();
  window.history.replaceState({}, "", "/");
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  if (serviceWorkerDescriptor)
    Object.defineProperty(navigator, "serviceWorker", serviceWorkerDescriptor);
  else Reflect.deleteProperty(navigator, "serviceWorker");
});

function pushMessage(
  container: EventTarget,
  data: unknown,
  scriptUrl = "/ui/push/sw.js",
  deadline = Date.now() + 5000,
) {
  const postMessage = vi.fn();
  const event = new MessageEvent("message", {
    data: { ...(data as object), deadline_ms: deadline },
  });
  Object.defineProperty(event, "source", {
    value: { scriptURL: new URL(scriptUrl, window.location.href).href },
  });
  Object.defineProperty(event, "ports", { value: [{ postMessage }] });
  container.dispatchEvent(event);
  return postMessage;
}
it.each(["", "event=message&"])(
  "returns to an authorized Group with %s without inventing a message",
  async (event) => {
    window.history.replaceState({}, "", `/?${event}group=b&lang=ja`);
    let parse!: () => void,
      selected = "";
    const message = vi.fn().mockResolvedValue(undefined),
      error = vi.fn(),
      tab = vi.fn();
    const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
    function Probe({ loaded = false }: { loaded?: boolean }) {
      const [id, setId] = useState("a");
      selected = id;
      parse = useDeepLink({
        groups: loaded ? groups : [],
        groupsLoaded: loaded,
        selectedGroupId: id,
        setSelectedGroupId: setId,
        setActiveTab: tab,
        openChatWindow: message,
        showError: error,
      }).parseUrlDeepLink;
      return null;
    }
    root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    await act(async () => {
      parse();
      root.render(<Probe loaded />);
    });
    expect(selected).toBe("b");
    expect(error).not.toHaveBeenCalled();
    if (event) expect(message).toHaveBeenCalledWith("b", "message");
    else expect(message).not.toHaveBeenCalled();
  },
);

it("keeps a message link until it is applied, then follows manual Group selection", async () => {
  window.history.replaceState(
    { retained: true },
    "",
    "/?group=b&event=message&tab=chat&lang=ja#notes",
  );
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  let parse!: () => void;
  let select!: (id: string) => void;
  function Probe({ loaded = false }: { loaded?: boolean }) {
    const [id, setId] = useState("a");
    select = setId;
    parse = useDeepLink({
      groups: loaded ? groups : [],
      groupsLoaded: loaded,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => {
    parse();
    root.render(<Probe loaded />);
  });
  expect(message).toHaveBeenCalledExactlyOnceWith("b", "message");
  expect(new URLSearchParams(location.search).get("event")).toBe("message");

  setWorkspaceDirty(Symbol("unsaved editor"), true);
  await act(async () => requestWorkspaceNavigation(() => select("a")));
  expect(new URLSearchParams(location.search).get("group")).toBe("b");
  await act(async () => finishWorkspaceNavigation(false));
  expect(new URLSearchParams(location.search).get("event")).toBe("message");
  await act(async () => {
    requestWorkspaceNavigation(() => select("a"));
    finishWorkspaceNavigation(true);
  });
  const params = new URLSearchParams(location.search);
  expect(params.get("group")).toBe("a");
  expect(params.has("event")).toBe(false);
  expect(params.has("tab")).toBe(false);
  expect(params.get("lang")).toBe("ja");
  expect(location.hash).toBe("#notes");
  expect(history.state).toEqual({ retained: true });
  expect(message).toHaveBeenCalledTimes(1);
  expect(error).not.toHaveBeenCalled();
});

it("repairs an invalid Group link to the selected fallback without replaying its message", async () => {
  window.history.replaceState({}, "", "/?group=missing&event=old&tab=chat&lang=zh");
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  const groups = [{ group_id: "a" }] as GroupMeta[];
  let parse!: () => void;
  function Probe({ loaded = false }: { loaded?: boolean }) {
    parse = useDeepLink({
      groups: loaded ? groups : [],
      groupsLoaded: loaded,
      selectedGroupId: "a",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => {
    parse();
    root.render(<Probe loaded />);
  });
  expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: missing");
  expect(message).not.toHaveBeenCalled();
  expect(location.search).toBe("?group=a&lang=zh");
});

it("opens a worker notification without discarding unsaved work", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  window.history.replaceState({}, "", "/?group=a");
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  const owner = Symbol("unsaved editor");
  function Probe() {
    const [id, setId] = useState("a");
    useDeepLink({
      groups,
      groupsLoaded: true,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  setWorkspaceDirty(owner, true);
  const data = { type: "cccc:push:open", v: 1, group_id: "a", event_id: "message-1" };
  let ack!: ReturnType<typeof vi.fn>;
  await act(async () => {
    ack = pushMessage(container, data);
  });
  expect(ack).toHaveBeenCalledExactlyOnceWith({ handled: true });
  expect(message).not.toHaveBeenCalled();
  expect(location.search).toBe("?group=a");
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  await act(async () => finishWorkspaceNavigation(false));
  expect(message).not.toHaveBeenCalled();

  await act(async () => pushMessage(container, data));
  await act(async () => finishWorkspaceNavigation(true));
  expect(message).toHaveBeenCalledExactlyOnceWith("a", "message-1");
  expect(new URLSearchParams(location.search).get("event")).toBe("message-1");
  expect(error).not.toHaveBeenCalled();
});

it("accepts only the dedicated worker and rechecks Group access before navigation", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  const owner = Symbol("unsaved editor");
  const both = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  const onlyA = [{ group_id: "a" }] as GroupMeta[];
  function Probe({ groups }: { groups: GroupMeta[] }) {
    const [id, setId] = useState("a");
    useDeepLink({
      groups,
      groupsLoaded: true,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe groups={both} />));
  const data = { type: "cccc:push:open", v: 1, group_id: "b", event_id: "message-2" };
  const wrongSource = pushMessage(container, data, "/another/sw.js");
  expect(wrongSource).not.toHaveBeenCalled();
  expect(message).not.toHaveBeenCalled();

  setWorkspaceDirty(owner, true);
  let ack!: ReturnType<typeof vi.fn>;
  await act(async () => {
    ack = pushMessage(container, data);
  });
  expect(ack).toHaveBeenCalledExactlyOnceWith({ handled: true });
  await act(async () => root.render(<Probe groups={onlyA} />));
  await act(async () => finishWorkspaceNavigation(true));
  expect(message).not.toHaveBeenCalled();
  expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: b");
});

it("keeps a notification until an earlier navigation confirmation is resolved", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  const message = vi.fn().mockResolvedValue(undefined);
  function Probe() {
    useDeepLink({
      groups: [{ group_id: "a" }] as GroupMeta[],
      groupsLoaded: true,
      selectedGroupId: "a",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: vi.fn(),
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  setWorkspaceDirty(Symbol("unsaved editor"), true);
  await act(async () => requestWorkspaceNavigation(vi.fn()));
  const previous = useWorkspaceNavigation.getState().pending;
  let ack!: ReturnType<typeof vi.fn>;
  await act(async () => {
    ack = pushMessage(container, {
      type: "cccc:push:open",
      v: 1,
      group_id: "a",
      event_id: "message-3",
    });
  });
  expect(ack).toHaveBeenCalledExactlyOnceWith({ handled: true });
  expect(useWorkspaceNavigation.getState().pending).toBe(previous);
  expect(message).not.toHaveBeenCalled();
  await act(async () => finishWorkspaceNavigation(false));
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  expect(useWorkspaceNavigation.getState().pending).not.toBe(previous);
  await act(async () => finishWorkspaceNavigation(true));
  expect(message).toHaveBeenCalledExactlyOnceWith("a", "message-3");
});

it("loads a notification's group when the page has not loaded its initial list yet", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups } });
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  function Probe() {
    const [list, setList] = useState<GroupMeta[]>([]);
    const [id, setId] = useState("a");
    useDeepLink({
      groups: list,
      groupsLoaded: false,
      setGroups: setList,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () =>
    pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "b", event_id: "message" }),
  );
  expect(apiJson).toHaveBeenCalledWith("/api/v1/groups", {
    cache: "no-store",
    signal: expect.any(AbortSignal),
  });
  expect(message).toHaveBeenCalledExactlyOnceWith("b", "message");
  expect(error).not.toHaveBeenCalled();
});

it.each(["worker", "URL"])(
  "refreshes a nonempty group list before rejecting a new %s target",
  async (source) => {
    const container = new EventTarget();
    Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
    window.history.replaceState({}, "", source === "URL" ? "/?group=b&event=message" : "/?group=a");
    const first = [{ group_id: "a" }] as GroupMeta[];
    const current = [...first, { group_id: "b" }] as GroupMeta[];
    vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups: current } });
    const message = vi.fn().mockResolvedValue(undefined);
    const error = vi.fn();
    let parse!: () => void;
    function Probe() {
      const [list, setList] = useState(first);
      const [id, setId] = useState("a");
      parse = useDeepLink({
        groups: list,
        groupsLoaded: true,
        setGroups: setList,
        selectedGroupId: id,
        setSelectedGroupId: setId,
        setActiveTab: vi.fn(),
        openChatWindow: message,
        showError: error,
      }).parseUrlDeepLink;
      return null;
    }
    root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    await act(async () => {
      if (source === "URL") parse();
      else
        pushMessage(container, {
          type: "cccc:push:open",
          v: 1,
          group_id: "b",
          event_id: "message",
        });
    });
    expect(apiJson).toHaveBeenCalledWith("/api/v1/groups", {
      cache: "no-store",
      signal: expect.any(AbortSignal),
    });
    expect(error).not.toHaveBeenCalled();
    expect(message).toHaveBeenCalledExactlyOnceWith("b", "message");
  },
);

it("refreshes the list for an ordinary message link to a newly visible Group", async () => {
  const first = [{ group_id: "a" }] as GroupMeta[];
  const current = [...first, { group_id: "b" }] as GroupMeta[];
  vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups: current } });
  const message = vi.fn().mockResolvedValue(undefined);
  const error = vi.fn();
  let open!: (groupId: string, eventId: string) => void;
  function Probe() {
    const [groups, setGroups] = useState(first);
    const [id, setId] = useState("a");
    open = useDeepLink({
      groups,
      groupsLoaded: true,
      setGroups,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    }).openMessageWindow;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => open("b", "message"));
  expect(apiJson).toHaveBeenCalledOnce();
  expect(error).not.toHaveBeenCalled();
  expect(message).toHaveBeenCalledExactlyOnceWith("b", "message");
});

it.each([
  [{ ok: true, result: { groups: [] } }, "Group not found: b"],
  [
    { ok: false, error: { code: "NETWORK_ERROR", message: "offline" } },
    "notifications.groupsUnavailable",
  ],
] as const)(
  "reports a missing group or an unreadable list without guessing",
  async (result, expected) => {
    const container = new EventTarget();
    Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
    vi.mocked(apiJson).mockResolvedValueOnce(result);
    const error = vi.fn();
    function Probe() {
      useDeepLink({
        groups: [],
        groupsLoaded: true,
        setGroups: vi.fn(),
        selectedGroupId: "",
        setSelectedGroupId: vi.fn(),
        setActiveTab: vi.fn(),
        openChatWindow: vi.fn(),
        showError: error,
      });

      return null;
    }
    root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    await act(async () =>
      pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "b", event_id: "message" }),
    );
    expect(error).toHaveBeenCalledExactlyOnceWith(expected);
  },
);

it.each(["worker", "URL"])(
  "reports an unavailable %s target only after checking a nonempty list",
  async (source) => {
    const container = new EventTarget();
    Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
    window.history.replaceState({}, "", source === "URL" ? "/?group=b&event=message" : "/?group=a");
    const groups = [{ group_id: "a" }] as GroupMeta[];
    vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups } });
    const error = vi.fn();
    let parse!: () => void;
    function Probe() {
      parse = useDeepLink({
        groups,
        groupsLoaded: true,
        setGroups: vi.fn(),
        selectedGroupId: "a",
        setSelectedGroupId: vi.fn(),
        setActiveTab: vi.fn(),
        openChatWindow: vi.fn(),
        showError: error,
      }).parseUrlDeepLink;
      return null;
    }
    root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    await act(async () => {
      if (source === "URL") parse();
      else
        pushMessage(container, {
          type: "cccc:push:open",
          v: 1,
          group_id: "b",
          event_id: "message",
        });
    });
    expect(apiJson).toHaveBeenCalledOnce();
    expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: b");
  },
);

it("waits for the page's first group list before resolving a URL target", async () => {
  window.history.replaceState({}, "", "/?group=b&event=message");
  const error = vi.fn();
  const message = vi.fn().mockResolvedValue(undefined);
  let parse!: () => void;
  function Probe({ list, loaded }: { list: GroupMeta[]; loaded: boolean }) {
    const [id, setId] = useState("");
    parse = useDeepLink({
      groups: list,
      groupsLoaded: loaded,
      setGroups: vi.fn(),
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe list={[]} loaded={false} />));
  await act(async () => parse());
  // Every reload carries the open Group: no lookup of its own, no error.
  expect(apiJson).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
  const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  await act(async () => root.render(<Probe list={groups} loaded />));
  expect(apiJson).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
  expect(message).toHaveBeenCalledExactlyOnceWith("b", "message");
});

it("reports a URL target missing from a loaded list that is empty", async () => {
  window.history.replaceState({}, "", "/?group=b&event=message");
  vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups: [] } });
  let parse!: () => void;
  const error = vi.fn();
  function Probe() {
    parse = useDeepLink({
      groups: [],
      groupsLoaded: true,
      setGroups: vi.fn(),
      selectedGroupId: "",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: vi.fn(),
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => parse());
  expect(apiJson).toHaveBeenCalledOnce();
  expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: b");
});

it("reports a link whose Group list cannot be read without mentioning notifications", async () => {
  window.history.replaceState({}, "", "/?group=b&event=message");
  vi.mocked(apiJson).mockResolvedValueOnce({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "offline" },
  });
  let parse!: () => void;
  const error = vi.fn();
  function Probe() {
    parse = useDeepLink({
      groups: [{ group_id: "a" }] as GroupMeta[],
      groupsLoaded: true,
      setGroups: vi.fn(),
      selectedGroupId: "a",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: vi.fn(),
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => parse());
  expect(apiJson).toHaveBeenCalledOnce();
  expect(error).toHaveBeenCalledExactlyOnceWith("notifications.groupsUnavailableForLink");
});

it("acts on a click that reaches a waking page late, but not on a stale or older one", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  const groups = [{ group_id: "a" }, { group_id: "b" }, { group_id: "c" }] as GroupMeta[];
  const message = vi.fn().mockResolvedValue(undefined);
  function Probe() {
    const [id, setId] = useState("a");
    useDeepLink({
      groups,
      groupsLoaded: true,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: vi.fn(),
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  const open = (group: string, deadline: number) =>
    act(async () =>
      pushMessage(
        container,
        { type: "cccc:push:open", v: 1, group_id: group, event_id: `in-${group}` },
        "/ui/push/sw.js",
        deadline,
      ),
    );
  // Past the worker's own deadline, as a frozen tab wakes: still taken.
  await open("b", Date.now() - 3000);
  expect(message).toHaveBeenLastCalledWith("b", "in-b");
  // Long past it: ignored.
  await open("c", Date.now() - 60000);
  // Older than the click already taken: ignored.
  await open("c", Date.now() - 5000);
  expect(message).toHaveBeenCalledOnce();
});

it("does not replace a newer group list with an older empty response", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  let finish!: (result: { ok: true; result: { groups: GroupMeta[] } }) => void;
  vi.mocked(apiJson).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  let latest: GroupMeta[] = [];
  const snapshot = () => latest;
  const setGroups = vi.fn();
  const error = vi.fn();
  const message = vi.fn().mockResolvedValue(undefined);
  function Probe({ list }: { list: GroupMeta[] }) {
    const [id, setId] = useState("a");
    useDeepLink({
      groups: list,
      groupsLoaded: true,
      setGroups,
      getGroupsSnapshot: snapshot,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe list={[]} />));
  await act(async () =>
    pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "a", event_id: "message" }),
  );
  latest = [{ group_id: "a" }] as GroupMeta[];
  await act(async () => finish({ ok: true, result: { groups: [] } }));
  expect(setGroups).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
  await act(async () => root.render(<Probe list={latest} />));
  expect(message).toHaveBeenCalledExactlyOnceWith("a", "message");
});

it("reports a stalled notification lookup instead of waiting indefinitely", async () => {
  vi.useFakeTimers();
  try {
    const container = new EventTarget();
    Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
    vi.mocked(apiJson).mockImplementation(
      async (_path, options) =>
        new Promise((resolve) => {
          options?.signal?.addEventListener("abort", () =>
            resolve({ ok: false, error: { code: "NETWORK_ERROR", message: "timed out" } }),
          );
        }),
    );
    const error = vi.fn();
    function Probe() {
      useDeepLink({
        groups: [],
        groupsLoaded: true,
        setGroups: vi.fn(),
        selectedGroupId: "",
        setSelectedGroupId: vi.fn(),
        setActiveTab: vi.fn(),
        openChatWindow: vi.fn(),
        showError: error,
      });
      return null;
    }
    root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    await act(async () =>
      pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "b", event_id: "message" }),
    );
    expect(error).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(10000));
    expect(error).toHaveBeenCalledExactlyOnceWith("notifications.groupsUnavailable");
  } finally {
    vi.useRealTimers();
  }
});

it("does not restore a Group removed while a notification lookup is in flight", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  let finishOld!: (result: { ok: true; result: { groups: GroupMeta[] } }) => void;
  const first = [{ group_id: "a" }] as GroupMeta[];
  const latest = [{ group_id: "a" }, { group_id: "c" }] as GroupMeta[];
  vi.mocked(apiJson)
    .mockReturnValueOnce(new Promise((resolve) => (finishOld = resolve)))
    .mockResolvedValueOnce({ ok: true, result: { groups: latest } });
  let snapshot = first;
  const setGroups = vi.fn();
  const error = vi.fn();
  function Probe({ list }: { list: GroupMeta[] }) {
    useDeepLink({
      groups: list,
      groupsLoaded: true,
      setGroups,
      getGroupsSnapshot: () => snapshot,
      selectedGroupId: "a",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: vi.fn(),
      showError: error,
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe list={first} />));
  await act(async () =>
    pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "b", event_id: "message" }),
  );
  snapshot = latest;
  await act(async () => root.render(<Probe list={latest} />));
  await act(async () =>
    finishOld({ ok: true, result: { groups: [...first, { group_id: "b" }] as GroupMeta[] } }),
  );
  expect(setGroups).not.toHaveBeenCalled();
  expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: b");
});

it("checks the current group store before following a URL from a stale render", async () => {
  window.history.replaceState({}, "", "/?group=b&event=message");
  const visible = [{ group_id: "a" }] as GroupMeta[];
  const rendered = [...visible, { group_id: "b" }] as GroupMeta[];
  vi.mocked(apiJson).mockResolvedValueOnce({ ok: true, result: { groups: visible } });
  const select = vi.fn();
  const error = vi.fn();
  let parse!: () => void;
  function Probe() {
    parse = useDeepLink({
      groups: rendered,
      groupsLoaded: true,
      getGroupsSnapshot: () => visible,
      setGroups: vi.fn(),
      selectedGroupId: "a",
      setSelectedGroupId: select,
      setActiveTab: vi.fn(),
      openChatWindow: vi.fn(),
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => parse());
  expect(select).not.toHaveBeenCalled();
  expect(error).toHaveBeenCalledExactlyOnceWith("Group not found: b");
});

it("lets a new notification supersede an unresolved URL lookup", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  window.history.replaceState({}, "", "/?group=a&event=older");
  let finishOld!: (result: { ok: true; result: { groups: GroupMeta[] } }) => void;
  vi.mocked(apiJson)
    .mockReturnValueOnce(new Promise((resolve) => (finishOld = resolve)))
    .mockResolvedValueOnce({ ok: true, result: { groups: [{ group_id: "b" }] } });
  let parse!: () => void;
  const error = vi.fn();
  const message = vi.fn().mockResolvedValue(undefined);
  function Probe() {
    // A loaded list without the link's Group, so its lookup is in flight.
    const [groups, setGroups] = useState<GroupMeta[]>([{ group_id: "c" }] as GroupMeta[]);
    const [id, setId] = useState("");
    parse = useDeepLink({
      groups,
      groupsLoaded: true,
      setGroups,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    }).parseUrlDeepLink;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => parse());
  await act(async () =>
    pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "b", event_id: "newer" }),
  );
  expect(message).toHaveBeenCalledExactlyOnceWith("b", "newer");
  await act(async () => finishOld({ ok: true, result: { groups: [] } }));
  expect(error).not.toHaveBeenCalled();
  expect(message).toHaveBeenCalledTimes(1);
});

it("lets a new same-Group message link supersede an unresolved URL lookup", async () => {
  window.history.replaceState({}, "", "/?group=b&event=older");
  let finishOld!: (result: { ok: true; result: { groups: GroupMeta[] } }) => void;
  vi.mocked(apiJson).mockReturnValueOnce(new Promise((resolve) => (finishOld = resolve)));
  const groups = [{ group_id: "a" }] as GroupMeta[];
  const error = vi.fn();
  const message = vi.fn().mockResolvedValue(undefined);
  let parse!: () => void;
  let open!: (groupId: string, eventId: string) => void;
  function Probe() {
    const link = useDeepLink({
      groups,
      groupsLoaded: true,
      setGroups: vi.fn(),
      selectedGroupId: "a",
      setSelectedGroupId: vi.fn(),
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: error,
    });
    parse = link.parseUrlDeepLink;
    open = link.openMessageWindow;
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => parse());
  await act(async () => {
    open("a", "newer");
    finishOld({ ok: true, result: { groups } });
  });
  expect(message).toHaveBeenCalledExactlyOnceWith("a", "newer");
  expect(error).not.toHaveBeenCalled();
});

it("uses the current group when a pending navigation is confirmed", async () => {
  const container = new EventTarget();
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: container });
  const groups = [{ group_id: "a" }, { group_id: "b" }] as GroupMeta[];
  const message = vi.fn().mockResolvedValue(undefined);
  let forceSelect!: (groupId: string) => void;
  function Probe() {
    const [id, setId] = useState("a");
    forceSelect = setId;
    useDeepLink({
      groups,
      groupsLoaded: true,
      selectedGroupId: id,
      setSelectedGroupId: setId,
      setActiveTab: vi.fn(),
      openChatWindow: message,
      showError: vi.fn(),
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  setWorkspaceDirty(Symbol("unsaved editor"), true);
  await act(async () =>
    pushMessage(container, { type: "cccc:push:open", v: 1, group_id: "a", event_id: "message" }),
  );
  await act(async () => forceSelect("b"));
  await act(async () => finishWorkspaceNavigation(true));
  expect(message).toHaveBeenCalledExactlyOnceWith("a", "message");
});
