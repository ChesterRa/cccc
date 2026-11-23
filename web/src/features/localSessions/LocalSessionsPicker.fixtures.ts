import enLayout from "../../i18n/locales/en/layout.json";
import enActors from "../../i18n/locales/en/actors.json";
import enCommon from "../../i18n/locales/en/common.json";
import zhLayout from "../../i18n/locales/zh/layout.json";
import zhActors from "../../i18n/locales/zh/actors.json";
import zhCommon from "../../i18n/locales/zh/common.json";
import jaLayout from "../../i18n/locales/ja/layout.json";
import jaActors from "../../i18n/locales/ja/actors.json";
import jaCommon from "../../i18n/locales/ja/common.json";
import type { DeviceSessionSource, GroupDoc, LocalSessionSource } from "../../types";

export const source: LocalSessionSource = {
  source_id: "codex:native",
  source_kind: "local",
  runtime: "codex",
  native_session_id: "native",
  title: "Codex · workspace",
  cwd: "/synthetic/project",
  connected: true,
};
export const second: DeviceSessionSource = {
  source_id: "remote:laptop:grant:second",
  source_kind: "device",
  runtime: "claude",
  node_id: "laptop",
  node_name: "laptop",
  title: "Claude · other workspace",
  cwd: "/synthetic/other",
  connected: true,
};
export const existingGroup: GroupDoc = {
  group_id: "g-existing",
  title: "Existing Group",
  state: "paused",
  running: false,
  runtime_status: {
    lifecycle_state: "paused",
    runtime_running: false,
    running_actor_count: 0,
    has_running_foreman: false,
  },
  active_scope_key: "original-scope",
  scopes: [{ scope_key: "original-scope", url: "/synthetic/original" }],
};

export const resources = {
  en: { layout: enLayout, actors: enActors, common: enCommon },
  zh: { layout: zhLayout, actors: zhActors, common: zhCommon },
  ja: { layout: jaLayout, actors: jaActors, common: jaCommon },
};
