// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useDeepLink } from "./useDeepLink";
import type { GroupMeta } from "../types";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: ReturnType<typeof createRoot>;
afterEach(async () => {
  await act(async () => root.unmount());
  window.history.replaceState({}, "", "/");
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
