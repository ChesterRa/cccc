import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vite-plus/test";
import { MessageContent } from "./MessageContent";

it("marks known references in a plain message", () => {
  const markup = renderToStaticMarkup(
    <MessageContent
      fallbackText={"@claude-1 see #34 and a@b.com"}
      shouldRenderMarkdown={false}
      isDark={false}
      mentionTokens={[{ text: "@claude-1", accent: 5 }]}
    />,
  );
  expect(markup).toContain(
    '<span data-message-mention="agent" data-mention-accent="5">@claude-1</span> see #34 and a@b.com',
  );
});
