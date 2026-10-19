// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useDeepLink } from "./useDeepLink";
import type { GroupMeta } from "../types";
import {
  finishWorkspaceNavigation,
  requestWorkspaceNavigation,
  setWorkspaceDirty,
  useWorkspaceNavigation,
} from "../stores/workspaceNavigation";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: ReturnType<typeof createRoot>;
afterEach(async () => {
  await act(async () => root.unmount());
  window.history.replaceState({}, "", "/");
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
});
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
