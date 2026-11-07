// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { MarkdownRenderer } from "./MarkdownRenderer";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

it("leaves text as written, so what is copied is what was sent", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);

  const text = '**见** ../up/y.md 和 v0.4.49..HEAD，a--b (c) "引用"';
  await act(async () => root!.render(<MarkdownRenderer content={text} />));

  expect(host.textContent).toContain('见 ../up/y.md 和 v0.4.49..HEAD，a--b (c) "引用"');
});
