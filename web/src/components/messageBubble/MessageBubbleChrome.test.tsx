import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { MessageFooter } from "./MessageBubbleChrome";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("MessageFooter", () => {
  it("keeps Mail visibly identified on each message row", () => {
    const markup = renderToStaticMarkup(
      <MessageFooter
        readOnly={false}
        obligationSummary={null}
        visibleReadStatusEntries={[]}
        readPreviewEntries={[]}
        readPreviewOverflow={0}
        displayNameMap={new Map()}
        isDark={false}
        isMail={true}
        replyRequested={false}
        copiedMessageText={false}
        copyableMessageText=""
        onCopyMessageText={() => undefined}
        onShowRecipients={() => undefined}
        onReply={() => undefined}
        canReply={false}
        event={{ id: "event-1", kind: "chat.message", by: "user", data: {} }}
      />,
    );

    expect(markup).toContain("mailMessageHint");
    expect(markup).toContain("modeMail");
  });
});
