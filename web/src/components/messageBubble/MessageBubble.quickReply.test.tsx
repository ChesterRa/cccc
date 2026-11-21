// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../../stores";
import {
  loadQuickReplyTemplates,
  resetQuickReplyTemplatesForTest,
} from "../../features/quickReplies/templateStore";
import type { QuickReplyEntry } from "../../features/quickReplies/effective";
import type { Actor, LedgerEvent } from "../../types";
import { MessageBubble } from "../MessageBubble";
import { MessageActions } from "./MessageActions";
import type { MessageActionsProps } from "./MessageActions";

/** The pointer bar and the touch menu, both as a message renders them. */
function BothParts(props: Omit<MessageActionsProps, "part">) {
  return (
    <>
      <MessageActions part="bar" {...props} />
      <MessageActions part="menu" {...props} />
    </>
  );
}

const replyMessage = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api")>()),
  replyMessage,
}));
const fetchQuickReplyTemplates = vi.hoisted(() => vi.fn());
vi.mock("../../services/api/quickReplyTemplates", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api/quickReplyTemplates")>()),
  fetchQuickReplyTemplates,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const DEFAULTS: QuickReplyEntry[] = [
  { builtin: "proceed" },
  { builtin: "why" },
  { builtin: "rethink" },
  { builtin: "choices" },
];
// The person's list: the given default entries (all four, unchanged, unless given), then theirs.
const templateList = (
  templates: { id: string; name: string; text: string }[],
  revision = 1,
  defaults: QuickReplyEntry[] = DEFAULTS,
) => ({
  ok: true,
  result: {
    replies: [...defaults, ...templates],
    revision,
    updated_at: "2026-11-15T00:00:00Z",
    limit: 24,
  },
});

const QUICK = ["quickReply.proceed", "quickReply.why", "quickReply.rethink", "quickReply.choices"];
const worker = { id: "worker", title: "Worker", runtime: "codex", enabled: true } as Actor;
const agentMessage: LedgerEvent = {
  id: "event-1",
  kind: "chat.message",
  by: "worker",
  group_id: "g",
  ts: "2026-11-03T00:00:00Z",
  data: { text: "Plan ready", to: ["user"] },
};

let host: HTMLDivElement;
let root: Root;
const menuItems = () =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'));
// An item's words, without the emoji that some lead with.
const labelOf = (item: Element) =>
  Array.from(item.childNodes)
    .filter((node) => !(node instanceof Element && node.hasAttribute("data-action-emoji")))
    .map((node) => node.textContent)
    .join("");
const menuGroups = () =>
  Array.from(document.querySelectorAll('[role="dialog"] [role="group"]')).map((group) =>
    group.getAttribute("aria-label"),
  );

async function renderBubble(event: LedgerEvent, extra: { readOnly?: boolean } = {}) {
  await act(async () =>
    root.render(
      <MessageBubble
        event={event}
        actorById={new Map([["worker", worker]])}
        actors={[worker]}
        displayNameMap={new Map([["worker", "Worker"]])}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        readOnly={extra.readOnly}
        groupId="g"
        groupLabelById={{}}
        onReply={vi.fn()}
        onShowRecipients={vi.fn()}
      />,
    ),
  );
}

async function touchMenuLabels() {
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button");
  if (!touch) return [];
  await act(async () => touch.click());
  const labels = menuItems().map(labelOf);
  await act(async () => touch.click());
  return labels;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  replyMessage.mockReset();
  fetchQuickReplyTemplates.mockReset();
  fetchQuickReplyTemplates.mockResolvedValue(templateList([], 0));
  resetQuickReplyTemplatesForTest();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("quick replies", () => {
  it("keep their own labelled part of the menus, before Copy link", async () => {
    const run = vi.fn();
    const quickReplies = QUICK.map((label) => ({ key: label, label, run }));
    const event = { id: "event-1", kind: "chat.message", by: "worker", data: {} };
    await act(async () =>
      root.render(
        <BothParts
          copyableMessageText="text"
          onCopyMessageText={vi.fn()}
          onCopyLink={vi.fn()}
          onRelay={vi.fn()}
          onReply={vi.fn()}
          canReply
          eventId={event.id}
          event={event}
          quickReplies={quickReplies}
        />,
      ),
    );
    const more = host.querySelector<HTMLButtonElement>(
      '[data-message-actions-inline] button[aria-label="messageActions"]',
    )!;
    await act(async () => more.click());
    expect(menuItems().map((item) => item.textContent)).toEqual([...QUICK, "copyLink"]);
    expect(menuGroups()).toEqual(["quickReply.menu", "messageActions"]);
    await act(async () => menuItems()[0].click());
    expect(run).toHaveBeenCalledOnce();
  });

  it("are offered on a message an agent of this Group wrote here, each led by its emoji", async () => {
    await renderBubble(agentMessage);
    expect(await touchMenuLabels()).toEqual(expect.arrayContaining(QUICK));
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    const quick = menuItems().filter((item) => QUICK.includes(labelOf(item)));
    const emoji = quick.map((item) => item.querySelector("[data-action-emoji]"));
    expect(emoji.map((mark) => mark?.textContent)).toEqual(["👍", "🤔", "🔄", "☑️"]);
    // Screen readers read the words and the group's name, which says a choice sends at once.
    for (const mark of emoji) expect(mark?.getAttribute("aria-hidden")).toBe("true");
    // Only the quick replies carry one.
    expect(menuItems().filter((item) => item.querySelector("[data-action-emoji]"))).toEqual(quick);
  });

  it.each([
    ["your own message", { ...agentMessage, by: "user" }],
    ["a member no longer in the Group", { ...agentMessage, by: "gone" }],
    ["Chet", { ...agentMessage, by: "svc:chet" }],
    ["a message still being written", { ...agentMessage, _streaming: true }],
    ["a question with its own answer card", { ...agentMessage, data: { text: "?", question: {} } }],
    // The daemon writes a message from another Group under that Group's ID and the sender's.
    [
      "a message from another Group",
      { ...agentMessage, by: "g2::worker", data: { text: "hi", src_group_id: "g2" } },
    ],
    // A plain send may also carry relay provenance, under the member's own ID.
    [
      "a member's relay from another Group",
      {
        ...agentMessage,
        data: { text: "hi", to: ["user"], src_group_id: "g2", src_event_id: "e9" },
      },
    ],
    // Member IDs often repeat across Groups, so the recipient there can share the author's ID.
    [
      "a message sent to another Group",
      {
        ...agentMessage,
        data: { text: "hi", to: ["user"], dst_group_id: "g2", dst_to: ["worker"] },
      },
    ],
    [
      "a message sent to another instance",
      { ...agentMessage, data: { text: "hi", dst_instance_id: "i2", dst_group_id: "g2" } },
    ],
    [
      "an IM message",
      { ...agentMessage, by: "user", data: { text: "hi", source_platform: "telegram" } },
    ],
  ])("are not offered on %s", async (_name, event) => {
    await renderBubble(event as LedgerEvent);
    const labels = await touchMenuLabels();
    for (const label of QUICK) expect(labels).not.toContain(label);
  });

  it("are not offered in a read-only view", async () => {
    await renderBubble(agentMessage, { readOnly: true });
    expect(host.querySelector(".message-actions-menu button")).toBeNull();
  });

  it("send the ready text to the author as a reply and say so", async () => {
    const showNotice = vi.fn();
    useUIStore.setState({ showNotice });
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    await act(async () =>
      menuItems()
        .find((item) => labelOf(item) === "quickReply.proceed")!
        .click(),
    );
    expect(replyMessage).toHaveBeenCalledWith(
      "g",
      "quickReply.proceedText",
      ["worker"],
      "event-1",
      undefined,
      expect.any(String),
      [],
      "Plan ready",
      "send",
    );
    expect(showNotice).toHaveBeenCalledWith({ message: "quickReply.sent" });
  });

  it("offers to retry a reply whose result is unknown", async () => {
    const showNotice = vi.fn();
    useUIStore.setState({ showNotice });
    replyMessage.mockResolvedValue({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "Network request failed" },
    });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    await act(async () =>
      menuItems()
        .find((item) => labelOf(item) === "quickReply.why")!
        .click(),
    );
    const notice = showNotice.mock.calls[0][0];
    expect(notice).toMatchObject({
      message: "quickReply.unknown",
      actionLabel: "quickReply.retry",
    });
    // The retry still works once the row is gone, as a virtual list unmounts rows.
    await act(async () => root.render(<div />));
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    await act(async () => notice.onAction());
    expect(replyMessage).toHaveBeenCalledTimes(2);
    expect(replyMessage.mock.calls[1][5]).toBe(replyMessage.mock.calls[0][5]);
    expect(showNotice).toHaveBeenLastCalledWith({ message: "quickReply.sent" });
  });

  it("list the person's replies in their saved order, their own without an emoji", async () => {
    fetchQuickReplyTemplates.mockResolvedValue(
      templateList([
        { id: "t1", name: "Ship it", text: "Ship it now." },
        { id: "t2", name: "Tests first", text: "Add the tests first." },
      ]),
    );
    // The chat page reads them; a row only shows what was read.
    await act(async () => {
      await loadQuickReplyTemplates();
    });
    await renderBubble(agentMessage);
    expect(fetchQuickReplyTemplates).toHaveBeenCalledOnce();
    const labels = await touchMenuLabels();
    const start = labels.indexOf(QUICK[0]);
    expect(labels.slice(start, start + 6)).toEqual([...QUICK, "Ship it", "Tests first"]);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    const own = menuItems().find((item) => labelOf(item) === "Ship it")!;
    expect(own.querySelector("[data-action-emoji]")).toBeNull();
    // It keeps the default replies' emoji column, so the names line up.
    const proceed = menuItems().find((item) => labelOf(item) === QUICK[0])!;
    expect(own.firstElementChild?.textContent).toBe("");
    expect(own.firstElementChild?.getAttribute("aria-hidden")).toBe("true");
    expect(own.firstElementChild?.className).toBe(proceed.firstElementChild?.className);
    // The message's other actions have no emoji, so their group keeps its full width.
    const others = document.querySelector(
      '[role="dialog"] [role="group"][aria-label="messageActions"]',
    )!;
    for (const item of others.querySelectorAll("button")) {
      expect(item.firstElementChild?.className).toContain("truncate");
    }
  });

  it("leave no emoji column when none of the person's replies has one", async () => {
    fetchQuickReplyTemplates.mockResolvedValue(
      templateList([{ id: "t1", name: "Ship it", text: "Ship it now." }], 1, [
        { builtin: "proceed", name: "Go" },
      ]),
    );
    await act(async () => {
      await loadQuickReplyTemplates();
    });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    for (const name of ["Go", "Ship it"]) {
      const item = menuItems().find((entry) => labelOf(entry) === name)!;
      expect(item.firstElementChild?.className).toContain("truncate");
    }
  });

  it("follow the saved order when a reply of the person's own comes before a default one", async () => {
    fetchQuickReplyTemplates.mockResolvedValue({
      ok: true,
      result: {
        replies: [{ id: "t1", name: "Ship it", text: "Ship it now." }, { builtin: "why" }],
        revision: 3,
        updated_at: "2026-11-15T00:00:00Z",
        limit: 24,
      },
    });
    await act(async () => {
      await loadQuickReplyTemplates();
    });
    await renderBubble(agentMessage);
    const labels = await touchMenuLabels();
    const start = labels.indexOf("Ship it");
    expect(labels.slice(start, start + 2)).toEqual(["Ship it", "quickReply.why"]);
    for (const name of ["quickReply.proceed", "quickReply.rethink", "quickReply.choices"]) {
      expect(labels).not.toContain(name);
    }
  });

  it("send a template's text as written and retry with that text after the template changes", async () => {
    const showNotice = vi.fn();
    useUIStore.setState({ showNotice });
    const written = "\n  Ship it now:\n    - run the gate\n";
    fetchQuickReplyTemplates.mockResolvedValue(
      templateList([{ id: "t1", name: "Ship it", text: written }]),
    );
    replyMessage.mockResolvedValue({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "Network request failed" },
    });
    await act(async () => {
      await loadQuickReplyTemplates();
    });
    await renderBubble(agentMessage);
    const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
    await act(async () => touch.click());
    await act(async () =>
      menuItems()
        .find((item) => labelOf(item) === "Ship it")!
        .click(),
    );
    expect(replyMessage.mock.calls[0][1]).toBe(written);
    const notice = showNotice.mock.calls[0][0];
    // The template is edited elsewhere before the retry; the retry repeats the first request.
    fetchQuickReplyTemplates.mockResolvedValue(
      templateList([{ id: "t1", name: "Ship it", text: "Changed." }], 2),
    );
    await act(async () => {
      await loadQuickReplyTemplates();
    });
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    await act(async () => notice.onAction());
    expect(replyMessage.mock.calls[1][1]).toBe(written);
    expect(replyMessage.mock.calls[1][5]).toBe(replyMessage.mock.calls[0][5]);
    expect(showNotice).toHaveBeenLastCalledWith({ message: "quickReply.sent" });
  });
});

it("sends a changed default, removes its original emoji and omits a deleted default", async () => {
  const body = "\n  Confirm the evidence first.\n";
  fetchQuickReplyTemplates.mockResolvedValue(
    templateList([], 1, [
      { builtin: "proceed", name: "Check first", text: body },
      { builtin: "rethink" },
      { builtin: "choices" },
    ]),
  );
  replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-customized" } } });
  await renderBubble(agentMessage);
  expect(fetchQuickReplyTemplates).not.toHaveBeenCalled();
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
  await act(async () => touch.click());
  expect(menuItems().map(labelOf)).not.toContain("quickReply.why");
  const changed = menuItems().find((item) => labelOf(item) === "Check first")!;
  expect(changed.querySelector("[data-action-emoji]")).toBeNull();
  await act(async () => changed.click());
  expect(replyMessage.mock.calls[0][1]).toBe(body);
});

it("disables first-read failures and recovers on the next deliberate menu opening", async () => {
  fetchQuickReplyTemplates.mockResolvedValue({
    ok: false,
    error: { code: "read_failed", message: "read failed" },
  });
  await renderBubble(agentMessage);
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
  await act(async () => touch.click());
  const unavailable = menuItems().find((item) => labelOf(item) === "replyTemplates.unavailable")!;
  expect(unavailable.disabled).toBe(true);
  for (const name of QUICK) expect(menuItems().map(labelOf)).not.toContain(name);
  await act(async () => unavailable.click());
  expect(replyMessage).not.toHaveBeenCalled();
  fetchQuickReplyTemplates.mockResolvedValue(
    templateList([], 1, [{ builtin: "proceed", name: "Recovered" }]),
  );
  await act(async () => touch.click());
  await act(async () => touch.click());
  expect(fetchQuickReplyTemplates).toHaveBeenCalledTimes(2);
  expect(menuItems().map(labelOf)).toContain("Recovered");
});

it("rereads an already loaded list when the menu opens and shows renamed or deleted replies", async () => {
  fetchQuickReplyTemplates.mockResolvedValue(
    templateList([], 1, [{ builtin: "proceed", name: "Old name" }, ...DEFAULTS.slice(1)]),
  );
  await act(async () => {
    await loadQuickReplyTemplates();
  });
  await renderBubble(agentMessage);
  expect(fetchQuickReplyTemplates).toHaveBeenCalledTimes(1);

  // Another device saved while this page remained open. Opening either entry reads again.
  fetchQuickReplyTemplates.mockResolvedValue(
    templateList([], 2, [
      { builtin: "proceed", name: "New name", text: "New body." },
      { builtin: "rethink" },
      { builtin: "choices" },
    ]),
  );
  const touch = host.querySelector<HTMLButtonElement>(".message-actions-menu button")!;
  await act(async () => touch.click());
  expect(fetchQuickReplyTemplates).toHaveBeenCalledTimes(2);
  expect(menuItems().map(labelOf)).toContain("New name");
  expect(menuItems().map(labelOf)).not.toContain("Old name");
  expect(menuItems().map(labelOf)).not.toContain("quickReply.why");
  replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-updated" } } });
  await act(async () =>
    menuItems()
      .find((item) => labelOf(item) === "New name")!
      .click(),
  );
  expect(replyMessage.mock.calls[0][1]).toBe("New body.");
  await act(async () => touch.click());
  expect(fetchQuickReplyTemplates).toHaveBeenCalledTimes(3);
});
