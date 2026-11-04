import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vite-plus/test";
import { getActorAccentIndex } from "../../types";
import { ComposerMentionOverlay } from "./ComposerMentionOverlay";

it("tints a chosen agent with its name colour and leaves selectors and Groups without one", () => {
  const text = "请 @Worker 和 @all 看 #Target Repo";
  const at = (token: string) => text.indexOf(token);
  const markup = renderToStaticMarkup(
    <ComposerMentionOverlay
      text={text}
      agentTokens={[
        {
          actorId: "worker",
          token: "@Worker",
          start: at("@Worker"),
          end: at("@Worker") + 7,
          scope: "selected",
        },
        {
          actorId: "@all",
          token: "@all",
          start: at("@all"),
          end: at("@all") + 4,
          scope: "selected",
        },
      ]}
      groupTokens={[]}
      scrollTop={0}
      compact={false}
      minHeight="2rem"
      maxHeight="10rem"
      fontSize={13}
      lineHeight={20}
    />,
  );
  expect(markup).toContain(
    `<span data-composer-mention="agent" data-mention-accent="${getActorAccentIndex("worker")}">@Worker</span>`,
  );
  expect(markup).toContain('<span data-composer-mention="agent">@all</span>');
});
