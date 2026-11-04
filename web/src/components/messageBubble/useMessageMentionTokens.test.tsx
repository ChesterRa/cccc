// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vite-plus/test";
import type { Actor } from "../../types";
import type { MessageMentionToken } from "../../utils/mentionHighlight";
import { useMessageMentionTokens } from "./useMessageMentionTokens";

it("keeps the same tokens while a replaced actor list has the same names", async () => {
  const seen: (readonly MessageMentionToken[])[] = [];
  function Probe({ actors }: { actors: Actor[] }) {
    seen.push(useMessageMentionTokens(actors, { refs: [] }));
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  const worker = { id: "worker", title: "Worker" } as Actor;
  await act(async () => root.render(<Probe actors={[worker]} />));
  await act(async () => root.render(<Probe actors={[{ ...worker, unread_count: 3 } as Actor]} />));
  await act(async () =>
    root.render(<Probe actors={[worker, { id: "reviewer", title: "评审员" } as Actor]} />),
  );
  await act(async () => root.unmount());
  expect(seen[1]).toBe(seen[0]);
  expect(seen[2]).not.toBe(seen[1]);
  expect(seen[2].map((token) => token.text)).toContain("@评审员");
});

it("keeps a name with a control character as one token", async () => {
  let tokens: readonly MessageMentionToken[] = [];
  function Probe() {
    tokens = useMessageMentionTokens(
      [{ id: "a", title: "name\u0000@phantom" } as Actor],
      undefined,
    );
    return null;
  }
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => root.unmount());
  const texts = tokens.map((token) => token.text);
  expect(texts).toContain("@name\u0000@phantom");
  expect(texts).not.toContain("@phantom");
});
