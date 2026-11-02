// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MessageBubble } from "./MessageBubble";
import { useGroupStore } from "../stores";
import type { GroupDoc, LedgerEvent } from "../types";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const group: GroupDoc = { group_id: "g1", generation: "generation-1" };
const fromChet = (marker: string): LedgerEvent => ({
  id: "e1",
  kind: "chat.message",
  by: "svc:chet",
  ts: "2026-11-02T08:00:00Z",
  data: { text: "Please check the build.", to: ["user"], _cccc_chet_group_generation: marker },
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  useGroupStore.setState({ groupDoc: group });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useGroupStore.setState({ groupDoc: null });
});

async function render(event: LedgerEvent) {
  await act(async () =>
    root.render(
      <MessageBubble
        event={event}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        groupId="g1"
        groupLabelById={{}}
        onReply={() => undefined}
        onShowRecipients={() => undefined}
      />,
    ),
  );
}

it("names Chet only on a message this Group's generation proves Chet wrote", async () => {
  await render(fromChet("generation-1"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("Chet");

  // A copied or re-created Group keeps the message but not the proof.
  await render(fromChet("generation-0"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("svc:chet");
});

it("shows the stored identity until the Group is loaded", async () => {
  useGroupStore.setState({ groupDoc: null });
  await render(fromChet("generation-1"));
  expect(host.querySelector(".lcd-row-name")?.textContent).toBe("svc:chet");
});
