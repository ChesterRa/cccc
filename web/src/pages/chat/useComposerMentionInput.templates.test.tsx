// @vitest-environment happy-dom
import { act, useRef, type Dispatch, type SetStateAction } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
} from "../../hooks/composerGroupMentions";
import { useComposerStore } from "../../stores/useComposerStore";
import { useComposerMentionInput } from "./useComposerMentionInput";

const noop = () => {};
const originalExecCommand = Object.getOwnPropertyDescriptor(document, "execCommand");
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let insert: (text: string) => void;

function setter<T>(read: () => T, write: (value: T) => void): Dispatch<SetStateAction<T>> {
  return (value) => write(typeof value === "function" ? (value as (old: T) => T)(read()) : value);
}

function Fixture() {
  const text = useComposerStore((state) => state.composerText);
  const input = useRef<HTMLTextAreaElement>(null);
  const api = useComposerMentionInput({
    text,
    composerRef: input,
    setText: setter(
      () => useComposerStore.getState().composerText,
      (value) => useComposerStore.getState().setComposerText(value),
    ),
    setGroupTokens: setter(
      () => useComposerStore.getState().composerGroupMentionTokens,
      (value) => useComposerStore.getState().setComposerGroupMentionTokens(value),
    ),
    setAgentTokens: setter(
      () => useComposerStore.getState().composerAgentMentionTokens,
      (value) => useComposerStore.getState().setComposerAgentMentionTokens(value),
    ),
    setShowMenu: noop,
    setSelectedIndex: noop,
    setFilter: noop,
    setKind: noop,
    setActorScope: noop,
    setTargetGroupId: noop,
    updateMenuPosition: noop,
  });
  insert = (value) =>
    api.insertTextAtSelection(value, (next, caret) => api.updateText(next, caret, caret));
  return (
    <>
      <textarea
        ref={input}
        value={text}
        onSelect={(event) => api.rememberSelection(event.currentTarget)}
        onChange={(event) =>
          api.updateText(
            event.target.value,
            event.target.selectionStart,
            event.target.selectionEnd,
            (event.nativeEvent as InputEvent).inputType,
          )
        }
      />
      <button type="button" onPointerDown={() => api.rememberSelection(input.current!)}>
        Choose template
      </button>
    </>
  );
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  useComposerStore.setState({
    composerText: "",
    composerGroupMentionTokens: [],
    composerAgentMentionTokens: [],
  });
  Object.defineProperty(document, "execCommand", { configurable: true, value: vi.fn(() => false) });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  if (originalExecCommand) Object.defineProperty(document, "execCommand", originalExecCommand);
  else Reflect.deleteProperty(document, "execCommand");
});

async function prepare(text: string, start: number, end = start) {
  useComposerStore.setState({ composerText: text });
  await act(async () => root.render(<Fixture />));
  const input = host.querySelector("textarea")!;
  input.focus();
  input.setSelectionRange(start, end);
  await act(async () => {
    host.querySelector("button")!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    host.querySelector("button")!.focus();
  });
  return input;
}

it("inserts multiline text at the remembered caret without changing the rest of the draft", async () => {
  const input = await prepare("before after", 7);
  await act(async () => insert("line 1\n  line 2\n"));
  expect(useComposerStore.getState().composerText).toBe("before line 1\n  line 2\nafter");
  expect(document.activeElement).toBe(input);
  expect(input.selectionStart).toBe("before line 1\n  line 2\n".length);
  expect(input.selectionEnd).toBe(input.selectionStart);
});

it("replaces the remembered selection and rebases both kinds of trailing mention", async () => {
  const text = "start replace @worker #group end";
  const agent = createComposerAgentMentionToken({
    actorId: "worker",
    token: "@worker",
    start: text.indexOf("@worker"),
    scope: "selected",
  })!;
  const group = createComposerGroupMentionToken({
    groupId: "g2",
    token: "#group",
    start: text.indexOf("#group"),
  })!;
  useComposerStore.setState({
    composerAgentMentionTokens: [agent],
    composerGroupMentionTokens: [group],
  });
  const input = await prepare(text, 6, 13);
  await act(async () => insert("new"));
  const state = useComposerStore.getState();
  expect(state.composerText).toBe("start new @worker #group end");
  expect(state.composerAgentMentionTokens).toEqual([
    { ...agent, start: agent.start - 4, end: agent.end - 4 },
  ]);
  expect(state.composerGroupMentionTokens).toEqual([
    { ...group, start: group.start - 4, end: group.end - 4 },
  ]);
  expect(input.selectionStart).toBe(9);
});

it("uses native insertText and the normal input event instead of replacing the controlled value", async () => {
  const input = await prepare("left old right", 5, 8);
  const fallbackText = "native";
  vi.mocked(document.execCommand).mockImplementation((command, _showUi, value) => {
    expect(command).toBe("insertText");
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(5);
    expect(input.selectionEnd).toBe(8);
    const next = input.value.slice(0, 5) + value + input.value.slice(8);
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, next);
    input.setSelectionRange(5 + value!.length, 5 + value!.length);
    input.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText", data: value }),
    );
    return true;
  });
  await act(async () => insert(fallbackText));
  expect(document.execCommand).toHaveBeenCalledOnce();
  expect(useComposerStore.getState().composerText).toBe("left native right");
  expect(input.selectionStart).toBe(11);
});

it("drops a replaced mention while preserving an untouched earlier reference", async () => {
  const text = "#group @worker end";
  const group = createComposerGroupMentionToken({ groupId: "g", token: "#group", start: 0 })!;
  const agent = createComposerAgentMentionToken({
    actorId: "worker",
    token: "@worker",
    start: 7,
    scope: "selected",
  })!;
  useComposerStore.setState({
    composerGroupMentionTokens: [group],
    composerAgentMentionTokens: [agent],
  });
  await prepare(text, 7, 14);
  await act(async () => insert("text"));
  expect(useComposerStore.getState().composerText).toBe("#group text end");
  expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([group]);
  expect(useComposerStore.getState().composerAgentMentionTokens).toEqual([]);
});

it("rebases surviving mentions when Undo restores a selected replacement or deletion, then Redo", async () => {
  const agent = createComposerAgentMentionToken({
    actorId: "worker",
    token: "@worker",
    start: 8,
    scope: "selected",
  })!;
  useComposerStore.setState({ composerAgentMentionTokens: [agent] });
  const input = await prepare("REPLACE @worker end", 7);
  const history = async (value: string, start: number, end: number, inputType: string) => {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        value,
      );
      input.setSelectionRange(start, end);
      input.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
    });
  };
  await history("before @worker end", 0, 6, "historyUndo");
  expect(useComposerStore.getState().composerAgentMentionTokens).toEqual([
    { ...agent, start: 7, end: 14 },
  ]);
  await history("REPLACE @worker end", 7, 7, "historyRedo");
  expect(useComposerStore.getState().composerAgentMentionTokens).toEqual([agent]);

  // Undo a deletion: the browser selects the restored text before the mention.
  await act(async () => {
    useComposerStore.setState({
      composerText: " @worker end",
      composerAgentMentionTokens: [{ ...agent, start: 1, end: 8 }],
    });
  });
  await prepare(" @worker end", 0);
  await history("before @worker end", 0, 6, "historyUndo");
  expect(useComposerStore.getState().composerAgentMentionTokens).toEqual([
    { ...agent, start: 7, end: 14 },
  ]);
});
