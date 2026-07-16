export interface ActorSecretChanges {
  setVars: Record<string, string>;
  unsetKeys: string[];
  clearAll: boolean;
}

export interface ActorSecretSaveChanges {
  setVars: Record<string, string>;
  unsetKeys: string[];
  clear: boolean;
}

export interface LoadedActorSecretKeys {
  keys: string[];
  masks: Record<string, string>;
  error: "";
  loadFailed: false;
}

const ENV_KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

export function emptyActorSecretChanges(): ActorSecretChanges {
  return { setVars: {}, unsetKeys: [], clearAll: false };
}

export function isValidActorSecretKey(key: string): boolean {
  return ENV_KEY_RE.test(String(key || "").trim());
}

export function normalizeLoadedActorSecretKeys(result?: {
  keys?: string[];
  masked_values?: Record<string, string>;
}): LoadedActorSecretKeys {
  return {
    keys: Array.isArray(result?.keys) ? result.keys : [],
    masks:
      result?.masked_values && typeof result.masked_values === "object" ? result.masked_values : {},
    error: "",
    loadFailed: false,
  };
}

export function stageActorSecretSet(
  changes: ActorSecretChanges,
  key: string,
  value: string,
): ActorSecretChanges {
  const normalizedKey = key.trim();
  return {
    ...changes,
    setVars: { ...changes.setVars, [normalizedKey]: value },
    unsetKeys: changes.unsetKeys.filter((item) => item !== normalizedKey),
  };
}

export function stageActorSecretSetMany(
  changes: ActorSecretChanges,
  setVars: Record<string, string>,
): ActorSecretChanges {
  return Object.entries(setVars).reduce(
    (next, [key, value]) => stageActorSecretSet(next, key, value),
    changes,
  );
}

export function stageActorSecretUnset(
  changes: ActorSecretChanges,
  key: string,
): ActorSecretChanges {
  const normalizedKey = key.trim();
  const { [normalizedKey]: _removed, ...setVars } = changes.setVars;
  return {
    ...changes,
    setVars,
    unsetKeys: changes.unsetKeys.includes(normalizedKey)
      ? [...changes.unsetKeys]
      : [...changes.unsetKeys, normalizedKey],
  };
}

export function undoActorSecretSet(changes: ActorSecretChanges, key: string): ActorSecretChanges {
  const { [key.trim()]: _removed, ...setVars } = changes.setVars;
  return { ...changes, setVars };
}

export function undoActorSecretUnset(changes: ActorSecretChanges, key: string): ActorSecretChanges {
  const normalizedKey = key.trim();
  return { ...changes, unsetKeys: changes.unsetKeys.filter((item) => item !== normalizedKey) };
}

export function setActorSecretClearAll(
  changes: ActorSecretChanges,
  clearAll: boolean,
): ActorSecretChanges {
  return { ...changes, clearAll };
}

export function buildActorSecretSaveChanges(changes: ActorSecretChanges): ActorSecretSaveChanges {
  if (changes.clearAll) {
    return { setVars: {}, unsetKeys: [], clear: true };
  }
  return { setVars: { ...changes.setVars }, unsetKeys: [...changes.unsetKeys], clear: false };
}
