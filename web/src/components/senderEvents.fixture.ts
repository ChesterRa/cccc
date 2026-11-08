import type { Actor, LedgerEvent } from "../types";

// Sender rows for the actor display-name tests, taken from a running group's
// ledger (one titled actor's message, a message from a sender outside the
// actor list, a user message, a system notice). Event ids, timestamps, kinds
// and field sets are as captured; actor ids, titles, recipients and text are
// redacted. Rows marked "mutation" change exactly one field of a captured row
// to cover a fallback the capture did not contain; they are not observed events.
export const capturedActors: Actor[] = [
  { id: "worker", title: "Review lead" } as Actor,
  // mutation: captured actor row with its title removed
  { id: "bare" } as Actor,
];

const userMessage: LedgerEvent = {
  id: "33ac4709bac7439e9a2e27e8dff88dd9",
  ts: "2026-09-21T16:47:15.722804Z",
  kind: "chat.message",
  by: "user",
  data: { format: "plain", message_mode: "send", text: "plan request", to: ["worker"] },
};

const withSender = (by: string, id: string): LedgerEvent => ({ ...userMessage, id, by });

export const capturedSenderEvents: LedgerEvent[] = [
  {
    id: "d11df72bff64457b9ec7a776dee0bcc5",
    ts: "2026-10-30T02:21:38.223293Z",
    kind: "chat.message",
    by: "worker",
    data: { format: "plain", message_mode: "request_reply", text: "review result", to: ["lead"] },
  },
  // mutation: captured user message sent by the untitled actor
  withSender("bare", "33ac4709bac7439e9a2e27e8dff88dd0"),
  userMessage,
  {
    id: "56f3dc56f68442a7a6b3e813f90f1d1e",
    ts: "2026-10-31T17:54:25.009591Z",
    kind: "chat.message",
    by: "ghost",
    data: { format: "plain", message_mode: "send", text: "status note", to: ["worker"] },
  },
  // mutations: captured user message with only the sender replaced
  withSender("svc:chet", "33ac4709bac7439e9a2e27e8dff88dd1"),
  withSender("@all", "33ac4709bac7439e9a2e27e8dff88dd2"),
  withSender("", "33ac4709bac7439e9a2e27e8dff88dd3"),
  {
    id: "be26929214bc4806aa338f7a56204fe3",
    ts: "2026-10-30T01:20:37.580150Z",
    kind: "system.notify",
    by: "system",
    data: {
      kind: "mail_notice",
      priority: "normal",
      title: "Mail waiting",
      message: "mail waiting",
    },
  },
];
