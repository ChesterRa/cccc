// Actual configuration dialogs with local props and a synthetic transport.
import { useState } from "react";
import { CreateGroupModal } from "../../src/components/modals/CreateGroupModal";
import { GroupEditModal } from "../../src/components/modals/GroupEditModal";
import { ActorConfigModal } from "../../src/components/modals/ActorConfigModal";
import type { SupportedRuntime } from "../../src/types";
import i18n from "../../src/i18n";
import "../../src/index.css";

const params = new URLSearchParams(location.search);
const isDark = params.get("theme") === "dark";
document.documentElement.classList.add(isDark ? "dark" : "light");
document.documentElement.style.fontSize = `${params.get("scale") || 100}%`;
await i18n.changeLanguage(params.get("lang") || "en");
const probe = { errors: [] as string[], actions: [] as string[], requests: [] as string[] };
Object.assign(window, { configurationWorkProbe: probe });
window.addEventListener("error", (e) => probe.errors.push(e.message));
window.addEventListener("unhandledrejection", (e) => probe.errors.push(String(e.reason)));
window.fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  probe.requests.push(`${init?.method || "GET"} ${url}`);
  return Response.json({
    ok: true,
    result: { keys: [], profiles: [], presets: [], items: [], packs: [], servers: [] },
  });
};
export function Fixture() {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("Release coordination");
  const [topic, setTopic] = useState("Review the current work before starting another iteration.");
  const [path, setPath] = useState("/workspace/project");
  const [actorId, setActorId] = useState("codex-1");
  const [role, setRole] = useState<"peer" | "foreman">("foreman");
  const [runtime, setRuntime] = useState<SupportedRuntime>("codex");
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
      {surface === "create-group" ? (
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
          groupId="g_fixture"
          groupRole="foreman"
          isRunning={false}
          title={title}
          onChangeTitle={setTitle}
          onSave={async () => saved()}
          onSaveAndRestart={async () => saved()}
        />
      )}
    </>
  );
}
