// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useVoiceDocumentLibrary } from "./useVoiceDocumentLibrary";
import { voiceDocumentLibrary } from "../../../services/api/voiceDocumentLibrary";
vi.mock("../../../services/api/voiceDocumentLibrary", () => ({ voiceDocumentLibrary: vi.fn() }));
let root: ReturnType<typeof createRoot>;
let state: ReturnType<typeof useVoiceDocumentLibrary>;
const empty = { folders: [], documents: [] };
function Harness({
  group = "a",
  documents = empty.documents,
}: {
  group?: string;
  documents?: unknown;
}) {
  state = useVoiceDocumentLibrary(group, documents, "");
  return null;
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  root = createRoot(document.createElement("div"));
  vi.mocked(voiceDocumentLibrary).mockReset().mockResolvedValue({ ok: true, result: empty });
});
afterEach(async () => {
  await act(async () => root.unmount());
});
it("keeps a pending mutation alive when the document list refreshes", async () => {
  await act(async () => root.render(<Harness />));
  let resolve!: (value: Awaited<ReturnType<typeof voiceDocumentLibrary>>) => void;
  vi.mocked(voiceDocumentLibrary).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  let pending!: Promise<boolean>;
  await act(async () => {
    pending = state.mutate({ action: "create_folder", name: "Folder" });
  });
  await act(async () => root.render(<Harness documents={[{ title: "refreshed" }]} />));
  await act(async () => {
    resolve({ ok: true, result: { ...empty, folders: [{ folder_id: "f", name: "Folder" }] } });
    await pending;
  });
  expect(state.busy).toBe(false);
  expect(state.data.folders[0]?.name).toBe("Folder");
});
it("ignores a previous group's late library response", async () => {
  let resolve!: (value: Awaited<ReturnType<typeof voiceDocumentLibrary>>) => void;
  vi.mocked(voiceDocumentLibrary).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await act(async () => root.render(<Harness group="a" />));
  await act(async () => root.render(<Harness group="b" />));
  await act(async () =>
    resolve({
      ok: true,
      result: { ...empty, folders: [{ folder_id: "private", name: "Group A" }] },
    }),
  );
  expect(state.data).toEqual(empty);
});
