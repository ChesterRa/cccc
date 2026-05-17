export type SseConnectionKind = "ledger" | "headless";

export type SseConnectionToken = {
  kind: SseConnectionKind;
  groupId: string;
  generation: number;
};

type Closable = {
  close: () => void;
};

type RegistryEntry<T extends Closable> = SseConnectionToken & {
  source: T;
};

export function createSseConnectionRegistry<T extends Closable>() {
  let generation = 0;
  const entries = new Map<SseConnectionKind, RegistryEntry<T>>();

  function closeEntry(kind: SseConnectionKind) {
    const entry = entries.get(kind);
    if (!entry) return;
    entries.delete(kind);
    entry.source.close();
  }

  return {
    set(kind: SseConnectionKind, groupId: string, source: T): SseConnectionToken {
      closeEntry(kind);
      const token = { kind, groupId, generation: ++generation };
      entries.set(kind, { ...token, source });
      return token;
    },

    close(kind: SseConnectionKind) {
      closeEntry(kind);
    },

    closeGroup(groupId: string) {
      for (const entry of Array.from(entries.values())) {
        if (entry.groupId === groupId) closeEntry(entry.kind);
      }
    },

    closeAll() {
      for (const kind of Array.from(entries.keys())) closeEntry(kind);
    },

    isCurrent(token: SseConnectionToken) {
      const entry = entries.get(token.kind);
      return !!entry && entry.groupId === token.groupId && entry.generation === token.generation;
    },
  };
}
