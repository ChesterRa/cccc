import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
import { expect, it } from "vite-plus/test";
import { MOBILE_VIEWPORT_MEDIA_QUERY } from "../utils/responsiveLayout";

const css = () =>
  readFileSync(fileURLToPath(new NodeURL("./chatMessage.css", import.meta.url)), "utf8");

it("keeps the members over the list clear of messages at rest and of text in passing", () => {
  const text = css();
  // The list ends with room for the members, so its last message rests above them.
  expect(text).toMatch(
    /\.chat-reading-scroller\.chat-list-over-dock \{\s*padding-bottom: max\(5rem, 72px\);\s*scroll-padding-bottom: max\(5rem, 72px\);/,
  );
  // Text scrolled under them fades into the chat surface before it reaches their keys.
  expect(text).toMatch(
    /\.chat-dock-fade \{[^}]*linear-gradient\(to bottom, transparent, var\(--color-chat-bg\) 1rem\),\s*linear-gradient\(to bottom, transparent, var\(--color-bg-primary\) 1rem\);/,
  );
  // Where their strip spans a phone's width, the key back to the latest rises above it, at
  // the breakpoint the page itself uses for a small screen.
  expect(text).toContain(
    `@media ${MOBILE_VIEWPORT_MEDIA_QUERY} {\n  .chat-list-over-dock ~ [data-scroll-latest] {\n    --chat-latest-bottom: calc(max(5rem, 72px) + 0.5rem);`,
  );
  expect(text.match(/bottom: var\(--chat-latest-bottom, [^)]+\);/g)).toHaveLength(2);
});
