// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vite-plus/test";

beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

it("remembers multiple sender identities and message types independently per Group after reload", async () => {
  const { useUIStore, getChatSession } = await import("./useUIStore");
  useUIStore.getState().setChatSenderFilter("a", ["peer", "leader"]);
  useUIStore.getState().setChatFilter("a", "mail");
  useUIStore.getState().setChatSenderFilter("b", ["worker"]);
  expect(getChatSession("new", useUIStore.getState().chatSessions).chatSenderFilter).toEqual([]);
  vi.resetModules();
  const reloaded = await import("./useUIStore");
  const sessions = reloaded.useUIStore.getState().chatSessions;
  expect(sessions.a).toMatchObject({ chatSenderFilter: ["leader", "peer"], chatFilter: "mail" });
  expect(sessions.b).toMatchObject({ chatSenderFilter: ["worker"], chatFilter: "all" });
  expect(sessions.a.scrollSnapshot).toBeNull();
});

it("retains old person selections and removes obsolete role selectors", async () => {
  localStorage.setItem(
    "cccc-chat-sessions",
    JSON.stringify({
      old: { chatFilter: "user" },
      invalid: { chatSenderFilter: "actor:" },
      person: { chatSenderFilter: "actor:worker" },
      human: { chatSenderFilter: "user" },
      role: { chatSenderFilter: "foreman" },
      multiple: { chatSenderFilter: ["peer", "leader", "peer", "", null, 12] },
    }),
  );
  const { useUIStore } = await import("./useUIStore");
  const sessions = useUIStore.getState().chatSessions;
  expect(sessions.old).toMatchObject({ chatFilter: "user", chatSenderFilter: [] });
  expect(sessions.invalid.chatSenderFilter).toEqual([]);
  expect(sessions.person.chatSenderFilter).toEqual(["worker"]);
  expect(sessions.human.chatSenderFilter).toEqual(["user"]);
  expect(sessions.role.chatSenderFilter).toEqual([]);
  expect(sessions.multiple.chatSenderFilter).toEqual(["leader", "peer"]);
});

it("normalizes the selected set and clears only the sender condition", async () => {
  const { useUIStore } = await import("./useUIStore");
  useUIStore.getState().setChatFilter("a", "mail");
  useUIStore.getState().setChatSenderFilter("a", ["worker", "leader", "worker"]);
  expect(useUIStore.getState().chatSessions.a.chatSenderFilter).toEqual(["leader", "worker"]);
  useUIStore.getState().setChatSenderFilter("a", []);
  expect(useUIStore.getState().chatSessions.a).toMatchObject({
    chatFilter: "mail",
    chatSenderFilter: [],
  });
});
