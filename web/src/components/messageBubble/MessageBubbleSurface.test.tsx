import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { MessageBubbleSurface } from "./MessageBubbleSurface";
import { MessageFooter } from "./MessageBubbleChrome";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function renderSurface(isUserMessage: boolean): string {
  return renderToStaticMarkup(
    <MessageBubbleSurface
      isUserMessage={isUserMessage}
      isStreaming={false}
      motionClass=""
      replyRequested={false}
      isHighlighted={false}
    >
      <p>Long product content remains inside the responsive message surface.</p>
    </MessageBubbleSurface>,
  );
}

describe("MessageBubbleSurface", () => {
  it("uses a neutral product surface for assistant messages", () => {
    const markup = renderSurface(false);

    expect(markup).toContain("max-w-full min-w-0");
    expect(markup).toContain("rounded-2xl");
    expect(markup).toContain("border-[var(--glass-border-subtle)]");
    expect(markup).toContain("shadow-[var(--glass-bubble-shadow)]");
    expect(markup).not.toContain("border-l-4");
    expect(markup).not.toMatch(/border-l-(?:sky|indigo|violet|fuchsia|cyan|teal|emerald|amber)/);
  });

  it("keeps the compact user bubble treatment", () => {
    const markup = renderSurface(true);

    expect(markup).toContain("glass-bubble");
    expect(markup).toContain("rounded-tr-md");
    expect(markup).toContain("min-w-[min(18rem,70vw)]");
  });
});

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
