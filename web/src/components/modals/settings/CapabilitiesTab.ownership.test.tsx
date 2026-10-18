// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, CapabilityOverviewItem } from "../../../types";
import { CAPABILITY_CHANGED_EVENT } from "../../../utils/capabilityEvents";
import { useSlashCommandState } from "../../../hooks/useSlashCommandState";

const api = vi.hoisted(() => ({
  fetchCapabilityOverview: vi.fn(),
  fetchGroups: vi.fn(),
  fetchSlashCommandCapabilityState: vi.fn(),
  fetchActors: vi.fn(),
  fetchGroupCapabilityState: vi.fn(),
  importCapability: vi.fn(),
  uninstallCapability: vi.fn(),
  enableGroupCapability: vi.fn(),
  updateActor: vi.fn(),
  updateGroupCapabilityVisibility: vi.fn(),
}));
vi.mock("../../../services/api", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import { CapabilitiesTab } from "./CapabilitiesTab";

const cap = "skill:agent_self_proposed:fixture";
const row = {
  capability_id: cap,
  kind: "skill",
  source_id: "agent_self_proposed",
  name: "Fixture Skill",
  capsule_text: "A synthetic reusable procedure",
  origin_group_id: "g1",
  qualification_status: "qualified",
} as CapabilityOverviewItem;
const overview = { ok: true, result: { items: [row] } };
const actor = {
  id: "a",
  role: "peer",
  title: "Actor A",
  runtime: "custom",
  capability_autoload: [cap],
  capability_hidden: [],
} as Actor;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

let host: HTMLDivElement;
let root: Root;
const render = (groupId = "g1") =>
  act(async () =>
    root.render(
      <CapabilitiesTab isDark={false} isActive groupId={groupId} surface="selfEvolving" />,
    ),
  );
const button = (label: string, scope: ParentNode = document) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((e) => e.textContent === label)!;
const click = (label: string) => act(async () => button(label).click());
const assignmentCheckboxes = () => [
  ...document.querySelectorAll<HTMLInputElement>('[role="dialog"] input[type="checkbox"]'),
];

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  api.fetchCapabilityOverview.mockResolvedValue(overview);
  api.fetchGroups.mockResolvedValue({ ok: true, result: { groups: [] } });
  api.fetchSlashCommandCapabilityState.mockResolvedValue({
    ok: true,
    result: { actor_hidden_capabilities: [] },
  });
  api.fetchActors.mockResolvedValue({ ok: true, result: { actors: [actor] } });
  api.fetchGroupCapabilityState.mockResolvedValue({
    ok: true,
    result: {
      capability_usage: {
        used: true,
        actor_enabled: [{ actor_id: "a" }],
        session_enabled: [{ actor_id: "a" }],
        group_enabled: false,
      },
    },
  });
  api.importCapability.mockResolvedValue({ ok: true, result: { record: row } });
  api.uninstallCapability.mockResolvedValue({ ok: true, result: {} });
  api.enableGroupCapability.mockImplementation(
    async (_gid: string, _cap: string, args: { enabled: boolean }) => ({
      ok: true,
      result: { enabled: args.enabled, state: args.enabled ? "enabled" : "disabled" },
    }),
  );
  api.updateActor.mockResolvedValue({
    ok: true,
    result: { actor: { ...actor, capability_autoload: [] } },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it.each(["save", "block", "unblock"] as const)(
  "keeps unsaved assignment and menu selections when the skill content action is %s",
  async (action) => {
    const record = { ...row, qualification_status: action === "unblock" ? "blocked" : "qualified" };
    api.fetchCapabilityOverview.mockResolvedValue({ ok: true, result: { items: [record] } });
    await render();
    await click("capabilities.selfProposedManage");
    expect(assignmentCheckboxes().map((e) => e.checked)).toEqual([true, false]);
    await act(async () => {
      for (const checkbox of assignmentCheckboxes()) checkbox.click();
    });
    await click(
      action === "save"
        ? "capabilities.manageSave"
        : action === "block"
          ? "capabilities.manageBlockSkill"
          : "capabilities.manageUnblockSkill",
    );
    expect(api.importCapability).toHaveBeenCalledTimes(1);
    expect(api.fetchActors).toHaveBeenCalledTimes(1);
    expect(api.fetchGroupCapabilityState).toHaveBeenCalledTimes(1);
    expect(assignmentCheckboxes().map((e) => e.checked)).toEqual([false, true]);
    expect(api.updateActor).not.toHaveBeenCalled();
    api.fetchActors
      .mockResolvedValueOnce({ ok: true, result: { actors: [actor] } })
      .mockResolvedValue({
        ok: true,
        result: { actors: [{ ...actor, capability_autoload: [], capability_hidden: [cap] }] },
      });
    api.fetchGroupCapabilityState.mockResolvedValue({
      ok: true,
      result: { capability_usage: { used: false, actor_hidden: [{ actor_id: "a" }] } },
    });
    await click("capabilities.manageSaveActorAssignments");
    expect(api.updateActor).toHaveBeenCalledWith(
      "g1",
      "a",
      undefined,
      undefined,
      undefined,
      expect.objectContaining({ capabilityAutoload: [], capabilityHidden: [cap] }),
    );
    expect(assignmentCheckboxes().map((e) => e.checked)).toEqual([false, true]);
    expect(api.fetchActors).toHaveBeenCalledTimes(3);
    expect(api.fetchGroupCapabilityState).toHaveBeenCalledTimes(2);
  },
);

it.each(["rejection", "error response"])(
  "retains assignment drafts when the post-assignment Actor refresh returns a %s",
  async (failure) => {
    await render();
    await click("capabilities.selfProposedManage");
    await act(async () => {
      for (const checkbox of assignmentCheckboxes()) checkbox.click();
    });
    // The write first reads Actors, then refreshes the saved assignments and usage.
    api.fetchActors.mockResolvedValueOnce({ ok: true, result: { actors: [actor] } });
    if (failure === "rejection") {
      api.fetchActors.mockRejectedValueOnce(new Error("Synthetic refresh failure"));
    } else {
      api.fetchActors.mockResolvedValueOnce({
        ok: false,
        error: { message: "Synthetic refresh failure" },
      });
    }
    await click("capabilities.manageSaveActorAssignments");
    expect(assignmentCheckboxes().map((e) => e.checked)).toEqual([false, true]);
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain(
      "Synthetic refresh failure",
    );
    expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(false);
  },
);

it("preserves a successful block when assignment save is clicked before the block returns, then content is saved", async () => {
  const blocked = deferred<void>();
  let stored = { ...row };
  api.fetchCapabilityOverview.mockImplementation(async () => ({
    ok: true,
    result: { items: [{ ...stored }] },
  }));
  api.importCapability.mockImplementation(async (_gid: string, record: CapabilityOverviewItem) => {
    if (api.importCapability.mock.calls.length === 1) await blocked.promise;
    stored = { ...record };
    return { ok: true, result: { record: { ...stored } } };
  });
  await render();
  await click("capabilities.selfProposedManage");
  await click("capabilities.manageBlockSkill");
  await click("capabilities.manageSaveActorAssignments");
  await act(async () => blocked.resolve());
  await click("capabilities.manageSave");
  expect(stored.qualification_status).toBe("blocked");
  expect(api.importCapability.mock.calls.map(([, record]) => record.qualification_status)).toEqual([
    "blocked",
    "blocked",
  ]);
  expect(api.enableGroupCapability).not.toHaveBeenCalled();
  expect(button("capabilities.manageUnblockSkill")).toBeDefined();
});

const managerWriteButtons = () =>
  [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].filter(
    (element) => element.textContent !== "capabilities.manageClose",
  );

it.each(["save", "block", "assign", "remove"])(
  "disables conflicting writes while %s is pending and still allows closing",
  async (action) => {
    const pending = deferred<{
      ok: true;
      result: { record: CapabilityOverviewItem; enabled: false };
    }>();
    const apiWrite =
      action === "assign"
        ? api.enableGroupCapability
        : action === "remove"
          ? api.uninstallCapability
          : api.importCapability;
    apiWrite.mockReturnValueOnce(pending.promise);
    vi.stubGlobal("confirm", () => true);
    await render();
    await click("capabilities.selfProposedManage");
    const label =
      action === "save"
        ? "capabilities.manageSave"
        : action === "block"
          ? "capabilities.manageBlockSkill"
          : action === "assign"
            ? "capabilities.manageSaveActorAssignments"
            : "capabilities.manageRemove";
    try {
      await click(label);
      expect(managerWriteButtons()).toHaveLength(4);
      expect(managerWriteButtons().every((element) => element.disabled)).toBe(true);
      expect(button("capabilities.manageClose").disabled).toBe(false);
      await click("capabilities.manageClose");
      expect(document.querySelector('[role="dialog"]')).toBeNull();
    } finally {
      await act(async () => pending.resolve({ ok: true, result: { record: row, enabled: false } }));
    }
  },
);

it.each(["save", "assign", "remove"])(
  "restores all write controls and keeps the draft after a failed %s",
  async (action) => {
    const pending = deferred<{ ok: false; error: { message: string } }>();
    const apiWrite =
      action === "assign"
        ? api.enableGroupCapability
        : action === "remove"
          ? api.uninstallCapability
          : api.importCapability;
    apiWrite.mockReturnValueOnce(pending.promise);
    vi.stubGlobal("confirm", () => true);
    await render();
    await click("capabilities.selfProposedManage");
    await act(async () => {
      for (const checkbox of assignmentCheckboxes()) checkbox.click();
    });
    await click(
      action === "save"
        ? "capabilities.manageSave"
        : action === "assign"
          ? "capabilities.manageSaveActorAssignments"
          : "capabilities.manageRemove",
    );
    await act(async () =>
      pending.resolve({ ok: false, error: { message: "Synthetic write failed" } }),
    );
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain(
      "Synthetic write failed",
    );
    expect(managerWriteButtons()).toHaveLength(4);
    expect(managerWriteButtons().every((element) => !element.disabled)).toBe(true);
    expect(assignmentCheckboxes().map((element) => element.checked)).toEqual([false, true]);
  },
);

it("waits for a closed manager's write before allowing another visit or a visibility write", async () => {
  const blocked = deferred<void>();
  let stored = { ...row };
  api.fetchCapabilityOverview.mockImplementation(async () => ({
    ok: true,
    result: { items: [{ ...stored }] },
  }));
  api.importCapability.mockImplementation(async (_gid: string, record: CapabilityOverviewItem) => {
    if (api.importCapability.mock.calls.length === 1) await blocked.promise;
    stored = { ...record };
    return { ok: true, result: { record: { ...stored } } };
  });
  await act(async () => root.render(<CapabilitiesTab isDark={false} isActive groupId="g1" />));
  await click("capabilities.selfProposedManage");
  await click("capabilities.manageBlockSkill");
  await click("capabilities.manageClose");
  try {
    expect(button("capabilities.selfProposedManage").disabled).toBe(true);
    expect(
      host.querySelector<HTMLButtonElement>(
        'button[aria-label="capabilities.hideFromSlashCommands"]',
      )!.disabled,
    ).toBe(true);
  } finally {
    await act(async () => blocked.resolve());
  }
  expect(button("capabilities.selfProposedManage").disabled).toBe(false);
  await click("capabilities.selfProposedManage");
  expect(button("capabilities.manageUnblockSkill")).toBeDefined();
  await click("capabilities.manageSave");
  expect(stored.qualification_status).toBe("blocked");
});

function SlashCatalogProbe() {
  const { slashCommands } = useSlashCommandState("g1");
  return <output data-slash-catalog>{slashCommands.map((item) => item.command).join(",")}</output>;
}

it("reconciles independent visibility writes that finish in reverse order, including the command catalog", async () => {
  const second = { ...row, capability_id: `${cap}-second`, name: "Second Skill" };
  const hidden = new Set<string>();
  const firstWrite = deferred<void>();
  const secondWrite = deferred<void>();
  api.fetchCapabilityOverview.mockResolvedValue({ ok: true, result: { items: [row, second] } });
  api.fetchSlashCommandCapabilityState.mockImplementation(async () => ({
    ok: true,
    result: {
      actor_hidden_capabilities: [...hidden],
      active_capsule_skills: [
        { capability_id: cap, name: "first" },
        { capability_id: second.capability_id, name: "second" },
      ],
    },
  }));
  api.updateGroupCapabilityVisibility.mockImplementation(async (_gid: string, id: string) => {
    await (id === cap ? firstWrite.promise : secondWrite.promise);
    hidden.add(id);
    return { ok: true, result: {} };
  });
  const changed = vi.fn();
  window.addEventListener(CAPABILITY_CHANGED_EVENT, changed);
  try {
    await act(async () =>
      root.render(
        <>
          <CapabilitiesTab isDark={false} isActive groupId="g1" />
          <SlashCatalogProbe />
        </>,
      ),
    );
    const controls = [
      ...host.querySelectorAll<HTMLButtonElement>(
        'button[aria-label="capabilities.hideFromSlashCommands"]',
      ),
    ];
    await act(async () => controls[0].click());
    await act(async () => controls[1].click());
    await act(async () => secondWrite.resolve());
    await act(async () => firstWrite.resolve());
    expect(
      host.querySelectorAll('button[aria-label="capabilities.showInSlashCommands"]'),
    ).toHaveLength(2);
    expect(host.querySelector("[data-slash-catalog]")!.textContent).not.toContain("/first");
    expect(host.querySelector("[data-slash-catalog]")!.textContent).not.toContain("/second");
    expect(changed).toHaveBeenCalledTimes(2);
  } finally {
    window.removeEventListener(CAPABILITY_CHANGED_EVENT, changed);
  }
});

it.each(["write", "overview"])(
  "keeps the latest visibility state when an earlier %s read finishes last",
  async (source) => {
    const second = { ...row, capability_id: `${cap}-second`, name: "Second Skill" };
    const olderRead = deferred<{ ok: true; result: { actor_hidden_capabilities: string[] } }>();
    api.fetchCapabilityOverview.mockResolvedValue({ ok: true, result: { items: [row, second] } });
    api.updateGroupCapabilityVisibility.mockResolvedValue({ ok: true, result: {} });
    await act(async () => root.render(<CapabilitiesTab isDark={false} isActive groupId="g1" />));
    api.fetchSlashCommandCapabilityState
      .mockReturnValueOnce(olderRead.promise)
      .mockResolvedValue({
        ok: true,
        result: { actor_hidden_capabilities: [cap, second.capability_id] },
      });
    const controls = [
      ...host.querySelectorAll<HTMLButtonElement>(
        'button[aria-label="capabilities.hideFromSlashCommands"]',
      ),
    ];
    if (source === "write") await act(async () => controls[0].click());
    else await click("capabilities.refresh");
    await act(async () => controls[1].click());
    await act(async () =>
      olderRead.resolve({ ok: true, result: { actor_hidden_capabilities: [cap] } }),
    );
    expect(
      host.querySelectorAll('button[aria-label="capabilities.showInSlashCommands"]'),
    ).toHaveLength(2);
  },
);

it("announces a completed visibility write to its original Group after leaving the page context", async () => {
  const written = deferred<{ ok: true; result: object }>();
  api.updateGroupCapabilityVisibility.mockReturnValue(written.promise);
  const changed = vi.fn();
  window.addEventListener(CAPABILITY_CHANGED_EVENT, changed);
  try {
    await act(async () => root.render(<CapabilitiesTab isDark={false} isActive groupId="g1" />));
    await act(async () =>
      host
        .querySelector<HTMLButtonElement>(
          'button[aria-label="capabilities.hideFromSlashCommands"]',
        )!
        .click(),
    );
    await act(async () => root.render(<CapabilitiesTab isDark={false} isActive groupId="g2" />));
    const reads = api.fetchSlashCommandCapabilityState.mock.calls.length;
    await act(async () => written.resolve({ ok: true, result: {} }));
    expect(changed).toHaveBeenCalledTimes(1);
    expect(changed.mock.calls[0][0].detail).toEqual({ group_id: "g1" });
    expect(api.fetchSlashCommandCapabilityState).toHaveBeenCalledTimes(reads);
    expect(
      host.querySelector('button[aria-label="capabilities.hideFromSlashCommands"]'),
    ).not.toBeNull();
  } finally {
    window.removeEventListener(CAPABILITY_CHANGED_EVENT, changed);
  }
});

it.each(["close", "group"] as const)(
  "completes removal of the original Actor grants after the manager is left via %s",
  async (leave) => {
    const update = deferred<{ ok: true; result: { actor: Actor } }>();
    api.updateActor.mockReturnValue(update.promise);
    await render();
    await click("capabilities.selfProposedManage");
    const checkbox = [
      ...document.querySelectorAll<HTMLInputElement>('[role="dialog"] input[type="checkbox"]'),
    ].find((e) => e.checked)!;
    await act(async () => checkbox.click());
    await click("capabilities.manageSaveActorAssignments");
    expect(api.updateActor).toHaveBeenCalledWith(
      "g1",
      "a",
      undefined,
      undefined,
      undefined,
      expect.objectContaining({ capabilityAutoload: [] }),
    );
    if (leave === "close") await click("capabilities.manageClose");
    else await render("g2");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await act(async () => {
      update.resolve({ ok: true, result: { actor: { ...actor, capability_autoload: [] } } });
      await update.promise;
    });
    for (const scope of ["actor", "session"]) {
      expect(api.enableGroupCapability).toHaveBeenCalledWith(
        "g1",
        cap,
        expect.objectContaining({ enabled: false, scope, actorId: "a" }),
      );
    }
    expect(api.enableGroupCapability.mock.calls.every(([gid]) => gid === "g1")).toBe(true);
  },
);

it("releases overview loading when a record save finishes during an older refresh", async () => {
  const oldRead = deferred<typeof overview>();
  api.fetchCapabilityOverview
    .mockResolvedValueOnce(overview)
    .mockReturnValueOnce(oldRead.promise)
    .mockResolvedValue(overview);
  await render();
  const refresh = button("capabilities.refresh", host);
  await act(async () => refresh.click());
  expect(refresh.disabled).toBe(true);
  await click("capabilities.selfProposedManage");
  await click("capabilities.manageSave");
  expect(api.importCapability).toHaveBeenCalledTimes(1);
  expect(api.fetchCapabilityOverview).toHaveBeenCalledTimes(3);
  await click("capabilities.manageClose");
  await act(async () => {
    oldRead.resolve(overview);
    await oldRead.promise;
  });
  expect(refresh.isConnected).toBe(true);
  expect(refresh.disabled).toBe(false);
});

it("loads Actor assignments after saving content while the first assignment read is pending", async () => {
  const actors = deferred<{ ok: true; result: { actors: Actor[] } }>();
  api.fetchActors.mockReturnValueOnce(actors.promise);
  await render();
  await click("capabilities.selfProposedManage");
  expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(true);
  await click("capabilities.manageSave");
  expect(api.importCapability).toHaveBeenCalledTimes(1);
  expect(api.fetchActors).toHaveBeenCalledTimes(1);
  expect(api.fetchGroupCapabilityState).toHaveBeenCalledTimes(1);
  expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(true);
  await act(async () => {
    actors.resolve({ ok: true, result: { actors: [actor] } });
    await actors.promise;
  });
  const dialog = document.querySelector('[role="dialog"]')!;
  expect(dialog.textContent).toContain("Actor A");
  expect(dialog.textContent).not.toContain("capabilities.manageUsageLoading");
  expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(false);
});

it("initializes assignments from the first usage read even if content is saved before it finishes", async () => {
  const usage = deferred<{
    ok: true;
    result: { capability_usage: { used: true; group_enabled: true } };
  }>();
  api.fetchActors.mockResolvedValue({
    ok: true,
    result: { actors: [{ ...actor, capability_autoload: [] }] },
  });
  api.fetchGroupCapabilityState.mockReturnValueOnce(usage.promise);
  await render();
  await click("capabilities.selfProposedManage");
  await click("capabilities.manageSave");
  expect(api.fetchActors).toHaveBeenCalledTimes(1);
  expect(api.fetchGroupCapabilityState).toHaveBeenCalledTimes(1);
  expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(true);
  await act(async () => {
    usage.resolve({ ok: true, result: { capability_usage: { used: true, group_enabled: true } } });
    await usage.promise;
  });
  expect(assignmentCheckboxes().map((e) => e.checked)).toEqual([true, false]);
  expect(button("capabilities.manageSaveActorAssignments").disabled).toBe(false);
});

it("does not refresh or refill a new Group from a departed record save", async () => {
  const saved = deferred<{ ok: true; result: { record: CapabilityOverviewItem } }>();
  api.importCapability.mockReturnValue(saved.promise);
  await render();
  await click("capabilities.selfProposedManage");
  await click("capabilities.manageSave");
  const next = {
    ...row,
    capability_id: "skill:agent_self_proposed:next",
    name: "Next Skill",
    origin_group_id: "g2",
  };
  api.fetchCapabilityOverview.mockResolvedValue({ ok: true, result: { items: [next] } });
  await render("g2");
  await click("capabilities.selfProposedManage");
  const reads = api.fetchCapabilityOverview.mock.calls.length;
  await act(async () => {
    saved.resolve({ ok: true, result: { record: row } });
    await saved.promise;
  });
  expect(api.fetchCapabilityOverview).toHaveBeenCalledTimes(reads);
  const dialog = document.querySelector('[role="dialog"]')!;
  expect(dialog.textContent).toContain("skill:agent_self_proposed:next");
  expect(dialog.textContent).not.toContain("capabilities.manageSaved");
  expect(
    [...dialog.querySelectorAll<HTMLInputElement>("input")].some((e) => e.value === "Next Skill"),
  ).toBe(true);
});

it("rolls back activation if startup persistence fails after the manager closes", async () => {
  const activation = deferred<{ ok: true; result: { enabled: true } }>();
  api.fetchActors.mockResolvedValue({
    ok: true,
    result: { actors: [{ ...actor, capability_autoload: [] }] },
  });
  api.fetchGroupCapabilityState.mockResolvedValue({
    ok: true,
    result: { capability_usage: { used: false } },
  });
  api.enableGroupCapability.mockImplementation(
    async (_gid: string, _cap: string, args: { enabled: boolean }) =>
      args.enabled ? activation.promise : { ok: true, result: { enabled: false } },
  );
  api.updateActor.mockResolvedValue({
    ok: false,
    error: { code: "fixture_failure", message: "Synthetic persistence failed" },
  });
  await render();
  await click("capabilities.selfProposedManage");
  const checkbox = document.querySelector<HTMLInputElement>(
    '[role="dialog"] input[type="checkbox"]',
  )!;
  await act(async () => checkbox.click());
  await click("capabilities.manageSaveActorAssignments");
  expect(api.enableGroupCapability).toHaveBeenCalledWith(
    "g1",
    cap,
    expect.objectContaining({ enabled: true, actorId: "a" }),
  );
  await click("capabilities.manageClose");
  await act(async () => {
    activation.resolve({ ok: true, result: { enabled: true } });
    await activation.promise;
  });
  expect(api.updateActor).toHaveBeenCalledWith(
    "g1",
    "a",
    undefined,
    undefined,
    undefined,
    expect.objectContaining({ capabilityAutoload: [cap] }),
  );
  expect(api.enableGroupCapability).toHaveBeenCalledWith(
    "g1",
    cap,
    expect.objectContaining({
      enabled: false,
      scope: "actor",
      actorId: "a",
      reason: "web_self_proposed_actor_assignment_rollback",
    }),
  );
});
