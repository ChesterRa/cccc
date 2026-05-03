import type { GlobalTabId, GroupTabId, SettingsScope } from "./types";

const STORAGE_KEY = "cccc.settings.lastLocation.v1";

const groupTabIds = new Set<GroupTabId>([
  "automation",
  "delivery",
  "guidance",
  "assistants",
  "space",
  "messaging",
  "im",
  "transcript",
  "blueprint",
]);

const globalTabIds = new Set<GlobalTabId>([
  "capabilities",
  "actorProfiles",
  "myProfiles",
  "branding",
  "webAccess",
  "webModels",
  "developer",
]);

export interface SettingsLastLocation {
  scope: SettingsScope;
  groupTab: GroupTabId;
  globalTab: GlobalTabId;
}

export const defaultSettingsLastLocation: SettingsLastLocation = {
  scope: "group",
  groupTab: "guidance",
  globalTab: "capabilities",
};

function normalizeScope(value: unknown): SettingsScope {
  return value === "global" ? "global" : "group";
}

function normalizeGroupTab(value: unknown): GroupTabId {
  return typeof value === "string" && groupTabIds.has(value as GroupTabId)
    ? (value as GroupTabId)
    : defaultSettingsLastLocation.groupTab;
}

function normalizeGlobalTab(value: unknown): GlobalTabId {
  return typeof value === "string" && globalTabIds.has(value as GlobalTabId)
    ? (value as GlobalTabId)
    : defaultSettingsLastLocation.globalTab;
}

export function readSettingsLastLocation(hasGroupScope: boolean): SettingsLastLocation {
  if (typeof window === "undefined") {
    return {
      ...defaultSettingsLastLocation,
      scope: hasGroupScope ? "group" : "global",
    };
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : {};
    const scope = normalizeScope(parsed?.scope);
    return {
      scope: scope === "group" && !hasGroupScope ? "global" : scope,
      groupTab: normalizeGroupTab(parsed?.groupTab),
      globalTab: normalizeGlobalTab(parsed?.globalTab),
    };
  } catch {
    return {
      ...defaultSettingsLastLocation,
      scope: hasGroupScope ? "group" : "global",
    };
  }
}

export function writeSettingsLastLocation(location: SettingsLastLocation): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(location));
  } catch {
    // Local storage can be unavailable in private or restricted browser contexts.
  }
}
