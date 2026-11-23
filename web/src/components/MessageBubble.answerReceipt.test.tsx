import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import type { LedgerEvent } from "../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      key === "answerReceiptSelected"
        ? `已选 ${values?.id}：`
        : key === "answerReceiptOther"
          ? "其他："
          : key === "answerReceiptNote"
            ? "备注："
            : key,
  }),
}));

function render(by: string, data: Record<string, unknown>) {
  const event: LedgerEvent = {
    id: `${by}-receipt`,
    kind: "chat.message",
    by,
    data: { message_mode: "send", to: ["worker"], reply_to: "request", ...data },
  };
  return renderToStaticMarkup(
    <MessageBubble
      event={event}
      actorById={new Map()}
      actors={[]}
      displayNameMap={new Map()}
      agentState={null}
      taskById={new Map()}
      isDark={false}
      groupId="g_local"
      groupLabelById={{}}
      onReply={() => undefined}
      onShowRecipients={() => undefined}
      onOpenSource={() => undefined}
    />,
  );
}

describe("answer receipts", () => {
  it("shows the user's receipt prefix in the reader's language", () => {
    const choice = render("user", { text: "Selected B: There", answer: { v: 1, option_id: "B" } });
    expect(choice).toContain("已选 B：There");
    expect(choice).not.toContain("Selected B:");
    const other = render("user", { text: "Other: Later", answer: { v: 1, text: "Later" } });
    expect(other).toContain("其他：Later");
  });

  it("localizes the choice and note of a recorded Chet answer", () => {
    const receipt = {
      text: "Selected B: There\n\nNote: After review.",
      answer: { v: 2, option_id: "B", note: "After review." },
      _cccc_chet_group_generation: "generation-1",
    };
    const delegated = render("svc:chet", { ...receipt, _cccc_chet_on_behalf_of: "user" });
    expect(delegated).toContain("已选 B：There");
    expect(delegated).toContain("备注：After review.");
    expect(delegated).not.toContain("Selected B:");
    expect(render("svc:chet", receipt)).toContain("Selected B: There");
  });

  it("leaves other senders and texts that do not match their answer as written", () => {
    expect(
      render("worker", { text: "Selected B: There", answer: { v: 1, option_id: "B" } }),
    ).toContain("Selected B: There");
    expect(
      render("user", { text: "Selected A: There", answer: { v: 1, option_id: "B" } }),
    ).toContain("Selected A: There");
  });
});
