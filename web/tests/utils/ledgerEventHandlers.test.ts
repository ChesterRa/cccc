import { describe, expect, it } from "vite-plus/test";
import { getActorRefreshMode } from "../../src/utils/ledgerEventHandlers";

describe("ledgerEventHandlers actor refresh mode", () => {
  it("treats chat.read as an unread refresh event", () => {
    expect(
      getActorRefreshMode({ kind: "chat.read", data: { actor_id: "peer1", event_id: "e1" } }),
    ).toBe("unread");
  });

  it("keeps system.notify as an unread refresh event", () => {
    expect(getActorRefreshMode({ kind: "system.notify", data: { target_actor_id: "peer1" } })).toBe(
      "unread",
    );
  });
});
