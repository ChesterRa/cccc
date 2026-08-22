import { describe, expect, it } from "vite-plus/test";
import { getActorRefreshMode } from "../../src/utils/ledgerEventHandlers";

describe("ledgerEventHandlers actor refresh mode", () => {
  it("treats mail.read as an unread refresh event", () => {
    expect(
      getActorRefreshMode({ kind: "mail.read", data: { actor_id: "peer1", event_id: "e1" } }),
    ).toBe("unread");
  });

  it("does not treat direct system notices as Mail unread state", () => {
    expect(getActorRefreshMode({ kind: "system.notify", data: { target_actor_id: "peer1" } })).toBe(
      "none",
    );
  });
});
