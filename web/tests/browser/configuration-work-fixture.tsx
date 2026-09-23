// Actual configuration dialogs with local props and a synthetic transport.
import { useModalStore } from "../../src/stores";
import { ModalFrame } from "../../src/components/modals/ModalFrame";
import { useModalA11y } from "../../src/hooks/useModalA11y";
import WebModelConnectorsTab from "../../src/components/modals/settings/WebModelConnectorsTab";
import { useState } from "react";
import { CreateGroupModal } from "../../src/components/modals/CreateGroupModal";
import { GroupEditModal } from "../../src/components/modals/GroupEditModal";
import { ActorConfigModal } from "../../src/components/modals/ActorConfigModal";
import { ActorProfilesTab } from "../../src/components/modals/settings/ActorProfilesTab";
import type { SupportedRuntime } from "../../src/types";
import i18n from "../../src/i18n";
import "../../src/index.css";

const params = new URLSearchParams(location.search);
const isDark = params.get("theme") === "dark";
document.documentElement.classList.add(isDark ? "dark" : "light");
document.documentElement.style.fontSize = `${params.get("scale") || 100}%`;
await i18n.changeLanguage(params.get("lang") || "en");
const probe = {
  errors: [] as string[],
  actions: [] as string[],
  requests: [] as string[],
  writes: [] as { url: string; body: unknown }[],
};
Object.assign(window, { configurationWorkProbe: probe });
window.addEventListener("error", (e) => probe.errors.push(e.message));
window.addEventListener("unhandledrejection", (e) => probe.errors.push(String(e.reason)));
window.fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  probe.requests.push(`${init?.method || "GET"} ${url}`);
  if (init?.body) probe.writes.push({ url, body: JSON.parse(String(init.body)) });
  return Response.json({
    ok: true,
    result: {
      keys: [],
      profiles: [],
      connectors: [],
      browser_session: { active: false },
      browser_surface: { active: false },
      pairing: { state: "bound", url: "https://chatgpt.com/c/fixture", actor_enabled: false },
      presets: [],
      items: [],
      packs: [],
      servers: [],
      groups: [],
      profile: { id: "fixture-profile", revision: 1 },
    },
  });
};
function SharedSettingsFixture() {
  const close = () => useModalStore.getState().closeModal("settings");
  const { modalRef } = useModalA11y(true, close);
  return (
    <ModalFrame
      isDark={isDark}
      onClose={close}
      titleId="fixture-shared-title"
      title="Shared settings fixture"
      closeAriaLabel="Close shared settings"
      panelClassName="h-[90dvh] w-full overflow-auto"
      modalRef={modalRef}
    >
      <WebModelConnectorsTab isDark={isDark} />
    </ModalFrame>
  );
}
export function Fixture() {
  const settingsOpen = useModalStore((state) => state.modals.settings);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("Release coordination");
  const [topic, setTopic] = useState("Review the current work before starting another iteration.");
  const [path, setPath] = useState("/workspace/project");
  const [actorId, setActorId] = useState("codex-1");
  const [role, setRole] = useState<"peer" | "foreman">("foreman");
  const [runtime, setRuntime] = useState<SupportedRuntime>(
    params.get("runtime") === "web_model" ? "web_model" : "codex",
  );
  const [command, setCommand] = useState("codex");
  const [notes, setNotes] = useState("");
  const [autoload, setAutoload] = useState("");
  const [secrets, setSecrets] = useState("");
  const [error, setError] = useState("");
  const [profileId, setProfileId] = useState("");
  const [useProfile, setUseProfile] = useState(false);
  const [useDefault, setUseDefault] = useState(true);
  const close = () => setOpen(false);
  const saved = () => {
    probe.actions.push("saved");
    close();
  };
  const base = { isOpen: open, isDark, busy: "", onCancel: close };
  const actor = {
    ...base,
    runtimes: [],
    actorProfiles: [],
    actorProfilesBusy: false,
    onSaveAsProfile: () => {},
    actorId,
    runtime,
    onChangeRuntime: setRuntime,
    command,
    onChangeCommand: setCommand,
    actorNotes: notes,
    onChangeActorNotes: setNotes,
    capabilityAutoloadText: autoload,
    onChangeCapabilityAutoloadText: setAutoload,
  };
  const surface = params.get("surface") || "create-group";
  return (
    <>
      <button id="open-configuration" onClick={() => setOpen(true)}>
        Open configuration
      </button>
      {surface === "runtime-profiles" ? (
        <ActorProfilesTab isDark={isDark} isActive scope="global" />
      ) : surface === "create-group" ? (
        <CreateGroupModal
          {...base}
          dirSuggestions={[]}
          dirItems={[]}
          currentDir={path}
          parentDir="/workspace"
          showDirBrowser={false}
          createGroupPath={path}
          setCreateGroupPath={setPath}
          createGroupName={title}
          setCreateGroupName={setTitle}
          creatingDirectory={false}
          onFetchDirContents={setPath}
          onCreateDirectory={async () => true}
          onCreateGroup={saved}
          onClose={close}
          onCancelAndReset={close}
        />
      ) : surface === "edit-group" ? (
        <GroupEditModal
          {...base}
          groupId="g_fixture"
          ccccHome="/fixture/state"
          projectRoot={path}
          title={title}
          topic={topic}
          onChangeTitle={setTitle}
          onChangeTopic={setTopic}
          onSave={saved}
          onReset={() => probe.actions.push("reset")}
          onDelete={() => probe.actions.push("delete")}
        />
      ) : surface === "create-actor" ? (
        <ActorConfigModal
          {...actor}
          mode="create"
          hasForeman={false}
          suggestedActorId="codex-1"
          onChangeActorId={setActorId}
          role={role}
          onChangeRole={setRole}
          useProfile={useProfile}
          onChangeUseProfile={setUseProfile}
          profileId={profileId}
          onChangeProfileId={setProfileId}
          useDefaultCommand={useDefault}
          onChangeUseDefaultCommand={setUseDefault}
          secretsSetText={secrets}
          onChangeSecretsSetText={setSecrets}
          error={error}
          onChangeError={setError}
          canSubmit={!error}
          submitDisabledReason=""
          onCreate={() => {
            saved();
            return true;
          }}
        />
      ) : (
        <ActorConfigModal
          {...actor}
          mode="edit"
          initialSection={params.get("focus") === "chatgpt" ? "chatgpt" : undefined}
          groupId="g_fixture"
          groupRole="foreman"
          isRunning={false}
          savedRuntime={params.get("runtime") === "web_model" ? "web_model" : "codex"}
          title={title}
          onChangeTitle={setTitle}
          onSave={async () => saved()}
          onSaveAndRestart={async () => saved()}
        />
      )}
      {settingsOpen && <SharedSettingsFixture />}
    </>
  );
}
