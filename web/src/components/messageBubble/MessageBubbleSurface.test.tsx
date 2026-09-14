import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vite-plus/test";

import { MessageBubbleSurface } from "./MessageBubbleSurface";

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

// This component's whole contract is which surface each sender gets, so the branch it picks is
// the only thing worth asserting — not the individual utilities inside either branch.
describe("MessageBubbleSurface", () => {
  it("gives the user a compact bubble and everyone else the neutral surface", () => {
    const user = renderSurface(true);
    expect(user).toContain("rounded-tr-md");
    expect(user).not.toContain("--glass-panel-bg");

    const assistant = renderSurface(false);
    expect(assistant).toContain("--glass-panel-bg");
    expect(assistant).not.toContain("rounded-tr-md");
    // Senders were once told apart by a coloured left border; the neutral surface replaced it.
    expect(assistant).not.toMatch(
      /border-l-(?:4|sky|indigo|violet|fuchsia|cyan|teal|emerald|amber)/,
    );
  });
});
