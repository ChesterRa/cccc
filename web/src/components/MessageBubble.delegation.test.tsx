import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("shows a delegation sent from this Group as a status, not as the relay prose", () => {
  const markup = renderToStaticMarkup(
    <MessageBubble
      event={{
        id: "relay",
        kind: "chat.message",
        by: "agent-a",
        data: {
          text: [
            "你好，我是来自「g_src」的 agent-a。用户让我来联系你。",
            "",
            "<!-- cccc-delegation-protocol",
            "[cccc-delegation:v1]",
            "delegation_id: dlg_1",
            "[/cccc-delegation]",
            "-->",
          ].join("\n"),
          to: ["target"],
          dst_group_id: "g_dst",
        },
      }}
      actorById={new Map()}
      actors={[]}
      displayNameMap={new Map()}
      agentState={null}
      taskById={new Map()}
      isDark={false}
      groupId="g_src"
      groupLabelById={{}}
      onReply={() => undefined}
      onShowRecipients={() => undefined}
      onOpenSource={() => undefined}
    />,
  );
  expect(markup).toContain("delegationOutboundStatus");
  expect(markup).not.toContain("用户让我来联系你");
  expect(markup).not.toContain("[cccc-delegation:v1]");
});
