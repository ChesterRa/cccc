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
    /\.chat-reading-scroller\.chat-list-over-dock \{\s*padding-bottom: max\(4\.5rem, 64px\);\s*scroll-padding-bottom: max\(4\.5rem, 64px\);/,
  );
  // The fade is as wide as the members' keys, so text on either side stays in view.
  expect(text).toMatch(
    /\.chat-dock-fade \{\s*position: relative;\s*width: fit-content;\s*max-width: 100%;\s*margin-inline: auto;\s*padding-top: 1rem;\s*\}/,
  );
  // Behind the keys, text scrolled under them fades into the chat surface, with soft ends.
  const fade = text.match(/\.chat-dock-fade::before \{([^}]*)\}/)?.[1];
  expect(fade).toMatch(/inset: 0 -1\.5rem;\s*z-index: -1;/);
  expect(fade).toMatch(
    /linear-gradient\(to bottom, transparent, var\(--color-chat-bg\) 1rem\),\s*linear-gradient\(to bottom, transparent, var\(--color-bg-primary\) 1rem\);/,
  );
  expect(
    fade?.match(/mask-image: linear-gradient\(\s*to right,\s*transparent,\s*#000 1\.5rem,/g),
  ).toHaveLength(2);
  // On a phone a line of agent activity makes the members taller: the room and the key back
  // to the latest follow their measured height (useFloatingDockReserve), at the breakpoint the
  // page itself uses for a small screen. Without that line the room stays as above.
  const flat = text.replace(/\s+/g, " ");
  const room = "max(4.5rem, 64px, calc(var(--chat-dock-height, 0px) + 0.75rem))";
  expect(flat).toContain(
    `@media ${MOBILE_VIEWPORT_MEDIA_QUERY} { .chat-reading-scroller.chat-list-over-dock { padding-bottom: ${room}; scroll-padding-bottom: ${room}; } }`,
  );
  expect(flat).toContain(
    `@media ${MOBILE_VIEWPORT_MEDIA_QUERY} { .chat-dock-shown ~ [data-scroll-latest] { --chat-latest-bottom: calc( ${room} + 0.5rem ); } }`,
  );
  expect(text.match(/bottom: var\(--chat-latest-bottom, [^)]+\);/g)).toHaveLength(2);
});
