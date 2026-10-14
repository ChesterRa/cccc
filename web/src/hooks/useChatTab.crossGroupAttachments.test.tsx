// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useComposerStore, useGroupStore, useUIStore } from "../stores";
import { useChatTab } from "./useChatTab";
import type { Actor } from "../types";

// Boundary: the HTTP helpers; the hook's own send guards and routing run for real.
vi.mock("../services/api", () => ({
  sendMessage: vi.fn(),
  replyMessage: vi.fn(),
  sendCrossGroupMessage: vi.fn(),
}));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({ items: [], loading: false }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const actors = [{ id: "peer-1", runtime: "custom", role: "peer", enabled: false } as Actor];
let root: Root;
let host: HTMLDivElement;
let chat: ReturnType<typeof useChatTab>;
function Harness() {
  const selectedGroupId = useGroupStore((s) => s.selectedGroupId);
  chat = useChatTab({
    selectedGroupId,
    selectedGroupRunning: false,
    actors,
    recipientActors: actors,
  });
  return null;
}

beforeEach(async () => {
  vi.clearAllMocks();
  useComposerStore.setState({ ...useComposerStore.getInitialState() });
  useGroupStore.setState({
    selectedGroupId: "g-a",
    groups: [],
    groupDoc: null,
    groupSettings: null,
    groupContext: null,
    chatByGroup: {},
  });
  useUIStore.setState({ chatSessions: {}, busy: "", errorMsg: "" });
  useComposerStore.getState().switchGroup(null, "g-a");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<Harness />);
  });
});
afterEach(async () => {
  await act(async () => {
    root.unmount();
  });
  host.remove();
});

it("sends composer files to another Group instead of refusing them", async () => {
  vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({ ok: true, result: {} });
  const file = new File(["png"], "chart.png", { type: "image/png" });
  await act(async () => {
    useComposerStore.getState().setDestGroupId("g-b");
    useComposerStore.getState().setComposerText("see chart");
    useComposerStore.getState().setComposerFiles([file]);
  });

  await act(async () => {
    await chat.sendMessage();
  });

  expect(useUIStore.getState().errorMsg).toBe("");
  expect(api.sendCrossGroupMessage).toHaveBeenCalledTimes(1);
  const [source, destination, text, , , files] = vi.mocked(api.sendCrossGroupMessage).mock.calls[0];
  expect([source, destination, text]).toEqual(["g-a", "g-b", "see chart"]);
  expect(files).toEqual([file]);
});
