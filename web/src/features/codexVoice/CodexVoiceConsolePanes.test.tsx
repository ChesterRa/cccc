// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { CodexVoiceConversationPane } from "./CodexVoiceConsolePanes";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./VoiceAcpConsole", () => ({ VoiceAcpConsole: () => null }));
vi.mock("./VoiceAnalystTerminal", () => ({ VoiceAnalystTerminal: () => null }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(state: { isEngaged?: boolean; isStarting?: boolean; conversation?: unknown[] }) {
  const controller = {
    conversation: state.conversation ?? [],
    isEngaged: state.isEngaged ?? false,
    isStarting: state.isStarting ?? false,
    checking: false,
    start: vi.fn(),
  };
  act(() =>
    root.render(
      <CodexVoiceConversationPane
        controller={controller as never}
        visible
        analystExpanded
        onToggleAnalyst={() => {}}
      />,
    ),
  );
  return controller;
}
const startButton = () =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("codexVoiceStart"));

it("keeps the empty conversation informational without duplicating the shell start action", () => {
  const controller = render({});
  expect(host.textContent).toContain("codexVoiceConversationEmptyTitle");
  expect(startButton()).toBeUndefined();
  expect(controller.start).not.toHaveBeenCalled();
});

it("does not start another call when the conversation rerenders during startup", () => {
  const controller = render({ isStarting: true });
  expect(startButton()).toBeUndefined();
  expect(controller.start).not.toHaveBeenCalled();
});

it("shows listening instead of a start action during a call", () => {
  render({ isEngaged: true });
  expect(host.textContent).toContain("codexVoiceConversationListening");
  expect(startButton()).toBeUndefined();
});
