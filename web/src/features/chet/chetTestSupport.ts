import { vi } from "vite-plus/test";
import type { ChetConversation } from "./useChetConversation";

/** A loaded, empty Chet conversation for surfaces that only host it. */
export function chetConversationStub(overrides: Partial<ChetConversation> = {}): ChetConversation {
  return {
    items: [],
    loaded: true,
    skipped: 0,
    historyError: "",
    hasOlder: false,
    loadingOlder: false,
    loadOlder: vi.fn(async () => {}),
    status: null,
    draft: "",
    storageAvailable: true,
    setText: vi.fn(),
    send: vi.fn(),
    retry: vi.fn(),
    sendAgain: vi.fn(),
    resentIds: new Set(),
    putBack: vi.fn(),
    dismiss: vi.fn(),
    problems: new Map(),
    composerProblem: null,
    clearComposerProblem: vi.fn(),
    duplicateRisk: false,
    working: false,
    unread: false,
    ...overrides,
  };
}
