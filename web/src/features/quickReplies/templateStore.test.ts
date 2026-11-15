// @vitest-environment happy-dom
import { act, createElement, Fragment } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { clearAuthToken, setAuthToken } from "../../services/api/base";
import {
  loadQuickReplyTemplates,
  resetQuickReplyTemplatesForTest,
  saveQuickReplyTemplateList,
  useQuickReplyTemplateStore,
  usePreloadQuickReplyTemplates,
} from "./templateStore";

const api = vi.hoisted(() => ({ fetch: vi.fn(), save: vi.fn() }));
vi.mock("../../services/api/quickReplyTemplates", () => ({
  fetchQuickReplyTemplates: api.fetch,
  saveQuickReplyTemplates: api.save,
}));

type Answer = {
  ok: true;
  result: {
    templates: { id: string; name: string; text: string }[];
    revision: number;
    updated_at: string;
    limit: number;
  };
};

const answer = (name: string, revision: number): Answer => ({
  ok: true,
  result: {
    templates: [{ id: `id-${name}`, name, text: `${name} text` }],
    revision,
    updated_at: "",
    limit: 20,
  },
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const shown = () => {
  const state = useQuickReplyTemplateStore.getState();
  return {
    names: state.templates.map((template) => template.name),
    revision: state.revision,
    loaded: state.loaded,
  };
};

beforeEach(() => {
  api.fetch.mockReset();
  api.save.mockReset();
  resetQuickReplyTemplatesForTest();
});

afterEach(() => {
  clearAuthToken();
});

describe("quick reply template store", () => {
  it("starts empty for a new sign-in and never shows an answer meant for the previous one", async () => {
    api.fetch.mockResolvedValueOnce(answer("A", 7));
    await loadQuickReplyTemplates();
    expect(shown()).toEqual({ names: ["A"], revision: 7, loaded: true });

    const lateRead = deferred<Answer>();
    const lateSave = deferred<Answer>();
    api.fetch.mockReturnValueOnce(lateRead.promise);
    api.save.mockReturnValueOnce(lateSave.promise);
    const readA = loadQuickReplyTemplates();
    const saveA = saveQuickReplyTemplateList(7, [{ name: "A2", text: "A2 text" }]);

    setAuthToken("token-b");
    expect(shown()).toEqual({ names: [], revision: 0, loaded: false });

    const readB = deferred<Answer>();
    api.fetch.mockReturnValueOnce(readB.promise);
    const loadB = loadQuickReplyTemplates();
    expect(api.fetch).toHaveBeenCalledTimes(3);

    // The previous person's answers arrive late, at higher revisions, while B's read is out.
    lateRead.resolve(answer("A", 8));
    lateSave.resolve(answer("A2", 9));
    await readA;
    await saveA;
    expect(shown()).toEqual({ names: [], revision: 0, loaded: false });
    expect(loadQuickReplyTemplates()).toBe(loadB);
    expect(api.fetch).toHaveBeenCalledTimes(3);

    readB.resolve(answer("B", 2));
    await loadB;
    expect(shown()).toEqual({ names: ["B"], revision: 2, loaded: true });
  });

  it("drops a read that started before a save was shown, and shows any later read as it is", async () => {
    api.fetch.mockResolvedValueOnce(answer("A", 1));
    await loadQuickReplyTemplates();
    const early = deferred<Answer>();
    api.fetch.mockReturnValueOnce(early.promise);
    const read = loadQuickReplyTemplates();
    api.save.mockResolvedValueOnce(answer("Saved", 2));
    await saveQuickReplyTemplateList(1, [{ name: "Saved", text: "Saved text" }]);
    early.resolve(answer("A", 1));
    await read;
    expect(shown()).toEqual({ names: ["Saved"], revision: 2, loaded: true });

    // A later read is the current list even at a lower revision: another tab may have signed
    // this browser in as someone else.
    api.fetch.mockResolvedValueOnce(answer("Other", 1));
    await loadQuickReplyTemplates();
    expect(shown()).toEqual({ names: ["Other"], revision: 1, loaded: true });
  });

  it("is read once by the page for all its rows, not on a read-only page, and again after a change of sign-in", async () => {
    api.fetch.mockResolvedValue(answer("A", 1));
    function Row() {
      const templates = useQuickReplyTemplateStore((state) => state.templates);
      return createElement("span", null, templates.map((template) => template.name).join(","));
    }
    function Page({ readOnly }: { readOnly: boolean }) {
      usePreloadQuickReplyTemplates(!readOnly);
      return createElement(Fragment, null, createElement(Row), createElement(Row));
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(createElement(Page, { readOnly: true }));
    });
    expect(api.fetch).not.toHaveBeenCalled();
    await act(async () => {
      root.render(createElement(Page, { readOnly: false }));
    });
    expect(api.fetch).toHaveBeenCalledTimes(1);
    expect(host.textContent).toBe("AA");

    api.fetch.mockResolvedValue(answer("B", 1));
    await act(async () => {
      clearAuthToken();
    });
    expect(api.fetch).toHaveBeenCalledTimes(2);
    expect(host.textContent).toBe("BB");

    act(() => root.unmount());
    host.remove();
  });
});
