import { expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { matchesChatFilters } from "./chatMessageFilters";

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
  expect(matchesChatFilters(reply, "request_reply", ["leader", "peer"])).toBe(true);
  expect(matchesChatFilters(reply, "mail", ["leader", "peer"])).toBe(false);
  expect(matchesChatFilters(reply, "user", [])).toBe(true);
  expect(matchesChatFilters(reply, "user", ["user"])).toBe(false);
  expect(matchesChatFilters({ ...reply, by: "user" }, "all", ["user", "peer"])).toBe(true);
  expect(matchesChatFilters(reply, "all", [])).toBe(true);
});
