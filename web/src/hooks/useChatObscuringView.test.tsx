// @vitest-environment happy-dom
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { groupMessagesVisible, useUIStore } from "../stores/useUIStore";
import { useChatObscuringView } from "./useChatObscuringView";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;

function CoveredView({ open }: { open: boolean }) {
  useChatObscuringView(open);
  return null;
}

beforeEach(() => {
  useUIStore.setState({
    activeTab: "chat",
    chatSessions: {},
    isSmallScreen: false,
    workspaceFileViewerGroupId: "",
    chatObscuringViews: 0,
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  expect(useUIStore.getState().chatObscuringViews).toBe(0);
  host.remove();
});

it("keeps chat hidden until the last covering view closes, including after a Group change", () => {
  const render = (first: boolean, second: boolean) =>
    act(() =>
      root.render(
        <>
          <CoveredView open={first} />
          <CoveredView open={second} />
        </>,
      ),
    );

  render(true, true);
  expect(useUIStore.getState().chatObscuringViews).toBe(2);
  expect(groupMessagesVisible("group-a", useUIStore.getState())).toBe(false);
  expect(groupMessagesVisible("group-b", useUIStore.getState())).toBe(false);

  render(true, false);
  expect(useUIStore.getState().chatObscuringViews).toBe(1);
  expect(groupMessagesVisible("group-b", useUIStore.getState())).toBe(false);

  render(false, false);
  expect(useUIStore.getState().chatObscuringViews).toBe(0);
  expect(groupMessagesVisible("group-b", useUIStore.getState())).toBe(true);
});

it("releases a covering view when it unmounts, including under StrictMode", () => {
  act(() =>
    root.render(
      <StrictMode>
        <CoveredView open />
      </StrictMode>,
    ),
  );
  expect(useUIStore.getState().chatObscuringViews).toBe(1);
  act(() => root.render(null));
  expect(useUIStore.getState().chatObscuringViews).toBe(0);
});

it("pairs each registration with one release even if the release is called twice", () => {
  const close = useUIStore.getState().registerChatObscuringView();
  const closeOther = useUIStore.getState().registerChatObscuringView();
  expect(useUIStore.getState().chatObscuringViews).toBe(2);
  close();
  close();
  expect(useUIStore.getState().chatObscuringViews).toBe(1);
  closeOther();
});
