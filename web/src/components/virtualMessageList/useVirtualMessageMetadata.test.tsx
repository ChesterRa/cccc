// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vite-plus/test";
import type { Actor, LedgerEvent } from "../../types";
import { useVirtualMessageMetadata } from "./useVirtualMessageMetadata";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const message = (id: string, by = "peer", data: Record<string, unknown> = {}) =>
  ({ id, kind: "chat.message", by, data: { text: id, ...data } }) as unknown as LedgerEvent;

// Every message row receives the name map. A new message without a Group Bridge alias keeps it,
// so the map alone no longer re-renders the rows; any change of a name shows in the same render.
it("keeps the display-name map for new messages and updates it when a name changes", async () => {
  const maps: Map<string, string>[] = [];
  function Probe({ messages, actors }: { messages: LedgerEvent[]; actors: Actor[] }) {
    maps.push(useVirtualMessageMetadata(messages, actors, []).displayNameMap);
    return null;
  }
  const peer = [{ id: "peer", title: "Peer" }] as Actor[];
  const root = createRoot(document.createElement("div"));
  const show = (messages: LedgerEvent[], actors = peer) =>
    act(async () => root.render(<Probe messages={messages} actors={actors} />));
  try {
    await show([message("m1")]);
    await show([message("m1"), message("m2")]);
    expect(maps[1]).toBe(maps[0]);

    await show([message("m1"), message("m2")], [{ id: "peer", title: "Renamed" }] as Actor[]);
    expect(maps[2].get("peer")).toBe("Renamed");
    expect(maps[1].get("peer")).toBe("Peer");
    await show([message("m1"), message("m2")], []);
    expect(maps[3].has("peer")).toBe(false);

    const bridged = message("m3", "group_bridge:x", { source_user_name: "Remote" });
    await show([message("m1"), bridged]);
    expect(maps[4].get("group_bridge:x")).toBe("Remote");
    expect(maps[4].get("peer")).toBe("Peer");
    await show([message("m1"), message("m2")]);
    expect(maps[5].has("group_bridge:x")).toBe(false);
  } finally {
    act(() => root.unmount());
  }
});
