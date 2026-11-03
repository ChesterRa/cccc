import { expect, it } from "vite-plus/test";
import { usePinStore } from "../../features/pins/pinStore";
import type { LedgerEvent } from "../../types";
import { matchesChatFilters, pinnedEmptyKey, shownChatFilter } from "./chatMessageFilters";

it("combines selected identities with OR and never matches a recipient or a similar name", () => {
  const senders = ["leader", "peer"];
  const event = { by: "leader", data: { to: ["other"] } } as LedgerEvent;
  expect(matchesChatFilters(event, "all", senders)).toBe(true);
  expect(matchesChatFilters({ ...event, by: "peer" }, "all", senders)).toBe(true);
  expect(
    matchesChatFilters(
      { ...event, by: "other", data: { to: ["leader"] } } as LedgerEvent,
      "all",
      senders,
    ),
  ).toBe(false);
  expect(matchesChatFilters({ ...event, by: "leader-2" }, "all", senders)).toBe(false);
  expect(matchesChatFilters({ ...event, by: "removed" }, "all", ["removed"])).toBe(true);
});

it("combines the sender set with the existing message categories using AND", () => {
  const reply = {
    by: "leader",
    data: { to: ["user"], message_mode: "request_reply" },
  } as LedgerEvent;
  expect(matchesChatFilters(reply, "user", ["leader", "peer"])).toBe(true);
  expect(matchesChatFilters(reply, "user", [])).toBe(true);
  expect(matchesChatFilters(reply, "user", ["user"])).toBe(false);
  expect(matchesChatFilters({ ...reply, by: "user" }, "all", ["user", "peer"])).toBe(true);
  expect(matchesChatFilters(reply, "all", [])).toBe(true);
});

it("only treats an authoritatively open user obligation as waiting", () => {
  const event: LedgerEvent = {
    kind: "chat.message",
    by: "worker",
    data: { to: ["user"], message_mode: "request_reply" },
    _user_request: { question: "none", requester: "available" },
    _obligation_status: {
      user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
    },
  };
  expect(matchesChatFilters(event, "pending_user", [])).toBe(true);
  expect(matchesChatFilters({ ...event, _user_request: undefined }, "pending_user", [])).toBe(
    false,
  );
  expect(
    matchesChatFilters(
      {
        ...event,
        _obligation_status: { user: { ...event._obligation_status!.user, replied: true } },
      },
      "pending_user",
      [],
    ),
  ).toBe(false);
});

it("matches Pinned by the Group's pins, combined with the sender set", () => {
  usePinStore.getState().replace("g1", [{ event_id: "e1", pinned_at: "2026-11-03T00:00:00Z" }]);
  const pinned = { id: "e1", group_id: "g1", by: "leader", data: {} } as LedgerEvent;
  expect(matchesChatFilters(pinned, "pinned", [])).toBe(true);
  expect(matchesChatFilters(pinned, "pinned", ["leader", "peer"])).toBe(true);
  expect(matchesChatFilters(pinned, "pinned", ["peer"])).toBe(false);
  expect(matchesChatFilters({ ...pinned, id: "e2" }, "pinned", [])).toBe(false);
  expect(matchesChatFilters({ ...pinned, group_id: "g2" }, "pinned", [])).toBe(false);
});

it("shows All for a stored filter whose tab is not offered", () => {
  const all = { waiting: true, pinned: true };
  expect(shownChatFilter("pinned", all)).toBe("pinned");
  expect(shownChatFilter("pending_user", all)).toBe("pending_user");
  expect(shownChatFilter("pinned", { ...all, pinned: false })).toBe("all");
  expect(shownChatFilter("pending_user", { ...all, waiting: false })).toBe("all");
  expect(shownChatFilter("user", { waiting: false, pinned: false })).toBe("user");
});

it("says a Group has no pinned messages only when it has none", () => {
  expect(pinnedEmptyKey(false, 0)).toBe("pins.empty");
  // Not known yet is not none.
  expect(pinnedEmptyKey(false, null)).toBeUndefined();
  // Two pins exist, but the sender filter hides them or they can no longer be read.
  expect(pinnedEmptyKey(false, 2)).toBeUndefined();
  expect(pinnedEmptyKey(true, 2)).toBe("pins.loadFailed");
});
