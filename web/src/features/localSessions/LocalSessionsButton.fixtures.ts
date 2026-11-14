import enLayout from "../../i18n/locales/en/layout.json";
import enActors from "../../i18n/locales/en/actors.json";
import enCommon from "../../i18n/locales/en/common.json";
import zhLayout from "../../i18n/locales/zh/layout.json";
import zhActors from "../../i18n/locales/zh/actors.json";
import zhCommon from "../../i18n/locales/zh/common.json";
import jaLayout from "../../i18n/locales/ja/layout.json";
import jaActors from "../../i18n/locales/ja/actors.json";
import jaCommon from "../../i18n/locales/ja/common.json";
import type { GroupDoc, LocalSessionSource } from "../../types";

export const source: LocalSessionSource = {
  source_id: "codex:native",
  runtime: "codex",
  native_session_id: "native",
  title: "Codex · workspace",
  cwd: "/synthetic/project",
  connected: true,
};
export const second: LocalSessionSource = {
  source_id: "claude:second",
  runtime: "claude",
  native_session_id: "second",
  title: "Claude · other workspace",
  cwd: "/synthetic/other",
  origin: "remote:laptop",
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

export const flowLabels = [
  [
    "en",
    "Connect sessions",
    "Sessions",
    "Create & connect",
    "Create & connect",
    "Selected sessions: 2 / 16",
    "Connect to Group",
  ],
  ["zh", "接入会话", "会话总览", "创建并接入", "创建并接入", "已选会话：2 / 16", "接入工作组"],
  [
    "ja",
    "セッションを接続",
    "セッション",
    "作成して接続",
    "作成して接続",
    "選択したセッション：2 / 16",
    "グループに接続",
  ],
];
