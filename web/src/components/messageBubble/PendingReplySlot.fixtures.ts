import type { LedgerEvent } from "../../types";

export const requestEvent: LedgerEvent = {
  id: "r1",
  group_id: "g1",
  kind: "chat.message",
  by: "peer",
  data: {
    to: ["user"],
    message_mode: "request_reply",
    text: "Question",
    question: {
      v: 1,
      prompt: "Question",
      options: [
        { id: "A", label: "Alpha" },
        { id: "B", label: "Beta" },
      ],
    },
  },
  _obligation_status: {
    user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
  },
  _user_request: { question: "valid", requester: "available" },
};
