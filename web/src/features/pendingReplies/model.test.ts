import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { createInstance } from "i18next";
import en from "../../i18n/locales/en/chat.json";
import ja from "../../i18n/locales/ja/chat.json";
import zh from "../../i18n/locales/zh/chat.json";
import {
  localizedAnswerReceipt,
  matchesSubmittedAnswer,
  parseQuestion,
  pendingReplyView,
  recordedOption,
  recordedQuestionText,
} from "./model";

const question = {
  v: 1,
  context: "Context",
  prompt: "Where?",
  options: [
    { id: "A", label: "Here", consequence: "Fast" },
    { id: "B", label: "There" },
  ],
  recommended: { option_id: "A", reason: "Fewer steps" },
};
const event: LedgerEvent = {
  id: "request",
  kind: "chat.message",
  by: "worker",
  data: { text: "Complete fallback", to: ["user"], message_mode: "request_reply", question },
};
const pending: LedgerEventStatusPayload = {
  obligation_status: {
    user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
  },
  user_request: { question: "valid", requester: "available" },
};

describe("pending user reply projection", () => {
  it("never treats missing status or another recipient as an open request", () => {
    expect(pendingReplyView(event)).toEqual({ state: "unknown", question: null });
    expect(
      pendingReplyView({ ...event, data: { message_mode: "request_reply", to: ["peer"] } }),
    ).toBeNull();
    expect(pendingReplyView({ ...event, by: "user" }, pending)).toBeNull();
  });

  it("uses the daemon's user obligation and validated question", () => {
    expect(pendingReplyView(event, pending)).toEqual({ state: "pending", question });
    const user = pending.obligation_status!.user;
    expect(
      pendingReplyView(event, {
        ...pending,
        obligation_status: { user: { ...user, replied: true } },
      })?.state,
    ).toBe("answered");
    expect(
      pendingReplyView(event, {
        ...pending,
        obligation_status: { user: { ...user, cancelled: true } },
      })?.state,
    ).toBe("withdrawn");
    expect(
      pendingReplyView(event, {
        ...pending,
        obligation_status: { user: { ...user, replied: true, cancelled: true } },
      })?.state,
    ).toBe("unknown");
  });

  it("retains explicit unavailable and unsupported requester states", () => {
    for (const requester of ["unavailable", "unknown", "unsupported"] as const) {
      expect(
        pendingReplyView(event, { ...pending, user_request: { question: "valid", requester } })
          ?.state,
      ).toBe(requester === "unknown" ? "requesterUnknown" : requester);
    }
  });

  it("uses full text for unknown question versions and never invents options", () => {
    const malformed = { ...event, data: { ...event.data, question: { ...question, v: 2 } } };
    expect(pendingReplyView(malformed, pending)).toEqual({ state: "unknown", question: null });
    expect(
      pendingReplyView(malformed, {
        ...pending,
        user_request: { question: "unavailable", requester: "available" },
      }),
    ).toEqual({ state: "pending", question: null });
  });

  it("rejects duplicated or invalid option IDs and recommendations", () => {
    expect(
      parseQuestion({ ...question, options: [question.options[0], question.options[0]] }),
    ).toBeNull();
    expect(
      parseQuestion({ ...question, recommended: { option_id: "C", reason: "No" } }),
    ).toBeNull();
    const option = (id: string) => ({ id, label: `Option ${id}` });
    expect(parseQuestion({ ...question, options: [option("A")] })).toBeNull();
    expect(
      parseQuestion({
        ...question,
        options: ["A", "B", "C", "D"].map(option),
        recommended: { option_id: "D", reason: "Last" },
      })?.options.map((item) => item.id),
    ).toEqual(["A", "B", "C", "D"]);
    expect(
      parseQuestion({ ...question, options: ["A", "B", "C", "D", "E"].map(option) }),
    ).toBeNull();
    expect(parseQuestion({ v: 1, prompt: "Text only", options: [] })).toEqual({
      v: 1,
      prompt: "Text only",
      options: [],
    });
    expect(parseQuestion({ v: 1, prompt: "Text only" })).toEqual({
      v: 1,
      prompt: "Text only",
      options: [],
    });
  });

  it("renders the recorded question without the instruction to answer", () => {
    const parsed = parseQuestion(question)!;
    const text = recordedQuestionText(parsed, "Recommended");
    expect(text).toBe(
      "Context\n\nWhere?\n\nA. Here — Fast\nB. There\nRecommended: A — Fewer steps",
    );
    expect(text).not.toContain("Reply with");
  });

  it("only accepts a reply that records the submitted answer and matching text", () => {
    const parsed = parseQuestion(question)!;
    const reply: LedgerEvent = {
      kind: "chat.message",
      by: "user",
      data: {
        reply_to: "request",
        to: ["worker"],
        message_mode: "send",
        answer: { v: 1, option_id: "B" },
        text: "Selected B: There",
      },
    };
    expect(recordedOption(parsed, "request", "worker", reply)).toMatchObject({
      id: "B",
      label: "There",
    });
    expect(matchesSubmittedAnswer(parsed, { v: 1, option_id: "B" }, reply)).toBe(true);
    const wrong = { ...reply, data: { ...reply.data, text: "Selected A: Here" } };
    expect(recordedOption(parsed, "request", "worker", wrong)).toBeNull();
    expect(matchesSubmittedAnswer(parsed, { v: 1, option_id: "B" }, wrong)).toBe(false);
    expect(matchesSubmittedAnswer(parsed, { v: 1, option_id: "A" }, reply)).toBe(false);
    const plain = { ...reply, data: { reply_to: "request", message_mode: "send", text: "words" } };
    expect(matchesSubmittedAnswer(null, { v: 1, text: "words" }, plain)).toBe(true);
    expect(matchesSubmittedAnswer(parsed, { v: 1, text: "words" }, plain)).toBe(false);
  });
});

describe("answer receipt prefixes", () => {
  const labels = { selected: (id: string) => `已选 ${id}：`, other: "其他：" };
  const receipt = (answer: unknown, text: string) =>
    localizedAnswerReceipt({ answer, text }, text, labels);

  it("localizes every option and a free answer beside options, without the question", () => {
    for (const id of ["A", "B", "C", "D"]) {
      expect(receipt({ v: 1, option_id: id }, `Selected ${id}: Label ${id}`)).toBe(
        `已选 ${id}：Label ${id}`,
      );
    }
    expect(receipt({ v: 1, text: "Later, with finance" }, "Other: Later, with finance")).toBe(
      "其他：Later, with finance",
    );
  });

  it("shows anything else as written", () => {
    // A free answer to a question without options has no prefix to replace.
    expect(receipt({ v: 1, text: "Just this" }, "Just this")).toBeNull();
    expect(receipt({ v: 2, option_id: "A" }, "Selected A: Here")).toBeNull();
    expect(receipt({ v: 1, option_id: "A", note: "x" }, "Selected A: Here")).toBeNull();
    expect(receipt({ v: 1, option_id: "A", text: "Here" }, "Selected A: Here")).toBeNull();
    expect(receipt({ v: 1, option_id: "" }, "Selected : Here")).toBeNull();
    expect(receipt({ v: 1, option_id: "B" }, "Selected A: Here")).toBeNull();
    expect(receipt({ v: 1, option_id: "A" }, "Selected A: ")).toBeNull();
    expect(receipt({ v: 1, text: "y" }, "Other: x")).toBeNull();
    expect(receipt("not an answer", "Selected A: Here")).toBeNull();
    expect(localizedAnswerReceipt(null, "Selected A: Here", labels)).toBeNull();
  });

  it.each(["en", "zh", "ja"])("has %s prefixes for the receipt", async (language) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      resources: { en: { translation: en }, zh: { translation: zh }, ja: { translation: ja } },
    });
    const selected = String(i18n.t("answerReceiptSelected", { id: "C" }));
    const other = String(i18n.t("answerReceiptOther"));
    expect(selected).toContain("C");
    expect(selected).not.toContain("answerReceipt");
    expect(other).not.toContain("answerReceipt");
    if (language === "en") {
      expect(`${selected}Third`).toBe("Selected C: Third");
      expect(`${other}Later`).toBe("Other: Later");
    }
  });
});
