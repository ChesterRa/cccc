// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { FailedSendsUnloadGuard } from "../../components/app/FailedSendsUnloadGuard";
import { useComposerStore, type FailedSend } from "../../stores/useComposerStore";
import { ComposerFailedSends } from "./ComposerFailedSends";

const nav = vi.hoisted(() => ({
  groups: [
    { group_id: "g-a", title: "Alpha" },
    { group_id: "g-b", title: "Billing" },
  ],
  setSelectedGroupId: vi.fn(),
  setActiveTab: vi.fn(),
}));
vi.mock("../../stores", () => {
  const groupState = () => ({
    groups: nav.groups,
    groupsLoaded: true,
    setSelectedGroupId: nav.setSelectedGroupId,
  });
  return {
    useGroupStore: Object.assign((select: (state: object) => unknown) => select(groupState()), {
      getState: groupState,
    }),
    useUIStore: { getState: () => ({ setActiveTab: nav.setActiveTab }) },
  };
});
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}:${JSON.stringify(options)}` : key,
  }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;

function failedSend(overrides: Partial<FailedSend>): FailedSend {
  return {
    id: "local-1",
    originGroupId: "g-a",
    destGroupId: "g-a",
    retryKey: "key",
    composerText: "",
    composerFiles: [],
    toText: "",
    replyTarget: null,
    quotedPresentationRef: null,
    quotedVoiceDocumentRef: null,
    messageMode: "send",
    ...overrides,
  };
}

function render(node: React.ReactNode) {
  act(() => root.render(node));
}

function rows() {
  return [...host.querySelectorAll<HTMLElement>("[data-failed-send]")];
}

function button(row: HTMLElement, label: string) {
  const found = [...row.querySelectorAll("button")].find((item) => item.textContent === label);
  if (!found) throw new Error(`missing ${label}`);
  return found;
}

beforeEach(() => {
  nav.groups = [
    { group_id: "g-a", title: "Alpha" },
    { group_id: "g-b", title: "Billing" },
  ];
  nav.setSelectedGroupId.mockClear();
  nav.setActiveTab.mockClear();
  useComposerStore.setState(useComposerStore.getInitialState(), true);
  useComposerStore.getState().switchGroup(null, "g-a");
  useComposerStore.setState({
    failedSendsByGroup: {
      "g-a": [
        failedSend({ id: "local-1", composerText: "First line\nsecond line" }),
        failedSend({
          id: "local-2",
          destGroupId: "g-b",
          composerText: "Invoice totals",
          composerFiles: [new File(["a"], "a.txt"), new File(["b"], "b.txt")],
        }),
      ],
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("folds the sends into one line that opens to the warning and the list", () => {
  render(<ComposerFailedSends groupId="g-a" isDark={false} />);
  const details = host.querySelector<HTMLDetailsElement>("[data-composer-failed-sends]");
  expect(details?.open).toBe(false);
  expect(details?.querySelector("summary")?.textContent).toBe('failedSendsSummary:{"count":2}');
  act(() => details?.querySelector("summary")?.click());
  expect(details?.open).toBe(true);
  // The uncertain result is written out, not left to a hover tip.
  expect(details?.querySelector("p")?.textContent).toBe("failedSendHint");
});

it("lists each send with its first line, another destination and its attachments", () => {
  useComposerStore.setState((state) => ({
    failedSendsByGroup: {
      "g-a": [
        ...state.failedSendsByGroup["g-a"],
        failedSend({
          id: "local-3",
          composerText: "Reply to another Group",
          replyTarget: { eventId: "e1", by: "peer", text: "hi", remoteDstGroupId: "g-remote" },
        }),
      ],
    },
  }));
  render(<ComposerFailedSends groupId="g-a" isDark={false} />);
  const [local, cross, remote] = rows();
  expect(rows()).toHaveLength(3);
  expect(local.textContent).toContain("First line");
  expect(local.textContent).not.toContain("second line");
  expect(local.textContent).not.toContain("failedSendTo");
  expect(cross.textContent).toContain('failedSendTo:{"group":"Billing"}');
  expect(cross.textContent).toContain('failedSendAttachments:{"count":2}');
  // An unknown Group still shows its ID rather than nothing.
  expect(remote.textContent).toContain('failedSendTo:{"group":"g-remote"}');
});

it("renders nothing while no Group has a failed send", () => {
  useComposerStore.setState({ failedSendsByGroup: {} });
  render(<ComposerFailedSends groupId="g-b" isDark={false} />);
  expect(host.querySelector("[data-composer-failed-sends]")).toBeNull();
});

it("lists another Group's sends wherever the user is, without their content", () => {
  render(<ComposerFailedSends groupId="g-b" isDark={false} />);
  expect(rows().map((row) => row.dataset.failedSendOrigin)).toEqual(["other", "other"]);
  const [first, second] = rows();
  expect(first.textContent).toContain('failedSendFrom:{"group":"Alpha"}');
  // Content shows only in the send's own Group, as drafts do.
  expect(first.textContent).not.toContain("First line");
  expect(second.textContent).not.toContain("Invoice totals");
  expect(second.textContent).not.toContain("failedSendAttachments");
  expect(host.textContent).not.toContain("failedSendUnavailableHint");
  // It is restored or discarded there, never from here.
  expect([...first.querySelectorAll("button")].map((item) => item.textContent)).toEqual([
    "failedSendOpen",
  ]);
  act(() => button(first, "failedSendOpen").click());
  expect(nav.setSelectedGroupId).toHaveBeenCalledWith("g-a");
  expect(nav.setActiveTab).toHaveBeenCalledWith("chat");
});

it("does not open a Group that left the list before the click", () => {
  render(<ComposerFailedSends groupId="g-b" isDark={false} />);
  nav.groups = [{ group_id: "g-b", title: "Billing" }];
  act(() => button(rows()[0], "failedSendOpen").click());
  expect(nav.setSelectedGroupId).not.toHaveBeenCalled();
});

it("shows a send whose Group is no longer listed without content and lets it be discarded", () => {
  nav.groups = [{ group_id: "g-b", title: "Billing" }];
  useComposerStore.setState({
    failedSendsByGroup: {
      "g-gone": [
        failedSend({
          id: "local-9",
          originGroupId: "g-gone",
          destGroupId: "g-gone",
          composerText: "Private draft",
          composerFiles: [new File(["a"], "a.txt")],
        }),
      ],
    },
  });
  render(<ComposerFailedSends groupId="g-b" isDark={false} />);
  const [row] = rows();
  expect(row.dataset.failedSendOrigin).toBe("unavailable");
  expect(row.textContent).toContain("failedSendSourceUnavailable");
  expect(row.textContent).not.toContain("Private draft");
  expect(row.textContent).not.toContain("failedSendAttachments");
  expect([...row.querySelectorAll("button")].map((item) => item.textContent)).toEqual([
    "failedSendDiscard",
  ]);
  // The limit is stated where the choice is made, not left to guesswork.
  expect(host.textContent).toContain("failedSendUnavailableHint");
  // Only restoring needs an empty composer.
  act(() => useComposerStore.getState().setComposerText("newer draft"));
  expect(host.textContent).not.toContain("failedSendRestoreBlocked");
  act(() => button(rows()[0], "failedSendDiscard").click());
  expect(host.querySelector("[data-composer-failed-sends]")).toBeNull();
});

it("hides content of the open Group's sends once that Group leaves the list", () => {
  // The Group list can refresh before the selection moves away from a removed Group.
  nav.groups = [{ group_id: "g-b", title: "Billing" }];
  render(<ComposerFailedSends groupId="g-a" isDark={false} />);
  expect(rows().map((row) => row.dataset.failedSendOrigin)).toEqual(["unavailable", "unavailable"]);
  expect(host.textContent).not.toContain("First line");
  expect(host.textContent).not.toContain("failedSendRestore");
});

it("keeps sends reachable with no Group open", () => {
  nav.groups = [{ group_id: "g-a", title: "Alpha" }];
  useComposerStore.setState((state) => ({
    failedSendsByGroup: {
      ...state.failedSendsByGroup,
      "g-gone": [failedSend({ id: "local-9", originGroupId: "g-gone", destGroupId: "g-gone" })],
    },
  }));
  render(<ComposerFailedSends groupId="" isDark={false} />);
  expect(rows().map((row) => row.dataset.failedSendOrigin)).toEqual([
    "other",
    "other",
    "unavailable",
  ]);
  expect(
    rows().map((row) => [...row.querySelectorAll("button")].map((b) => b.textContent)),
  ).toEqual([["failedSendOpen"], ["failedSendOpen"], ["failedSendDiscard"]]);
});

it("restores into an empty composer and blocks restore while a draft is present", () => {
  render(<ComposerFailedSends groupId="g-a" isDark={false} />);
  act(() => useComposerStore.getState().setComposerText("newer draft"));
  expect(button(rows()[0], "failedSendRestore").disabled).toBe(true);
  expect(host.textContent).toContain("failedSendRestoreBlocked");

  act(() => useComposerStore.getState().setComposerText(""));
  expect(host.textContent).not.toContain("failedSendRestoreBlocked");
  act(() => button(rows()[0], "failedSendRestore").click());
  expect(useComposerStore.getState().composerText).toBe("First line\nsecond line");
  expect(rows().map((row) => row.dataset.failedSend)).toEqual(["local-2"]);
});

it("discards one send and keeps the rest", () => {
  render(<ComposerFailedSends groupId="g-a" isDark={false} />);
  act(() => button(rows()[1], "failedSendDiscard").click());
  expect(rows().map((row) => row.dataset.failedSend)).toEqual(["local-1"]);
  expect(useComposerStore.getState().composerText).toBe("");
});

it("asks before leaving the page only while a send waits to be restored", () => {
  render(<FailedSendsUnloadGuard />);
  const leave = () => {
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  };
  expect(leave()).toBe(true);
  act(() => useComposerStore.setState({ failedSendsByGroup: {} }));
  expect(leave()).toBe(false);
});
