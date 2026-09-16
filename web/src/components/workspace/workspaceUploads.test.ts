// @vitest-environment happy-dom
import { describe, expect, it } from "vite-plus/test";
import { droppedEntries, pickerEntries, validateUploadEntries } from "./workspaceUploads";

describe("workspace upload selection", () => {
  it("preserves nested picker paths and rejects metadata before uploading", () => {
    const file = new File(["hello"], "name.txt");
    Object.defineProperty(file, "webkitRelativePath", { value: "tree/nested/name.txt" });
    expect(pickerEntries([file]).map((entry) => entry.path)).toEqual([
      "tree",
      "tree/nested",
      "tree/nested/name.txt",
    ]);
    expect(() => validateUploadEntries([{ path: "tree/.git/config", file }])).toThrow(
      "invalidPath",
    );
    expect(() => validateUploadEntries([{ path: "../escape", file }])).toThrow("invalidPath");
    expect(() =>
      validateUploadEntries([
        { path: "one", file },
        { path: "one", file },
      ]),
    ).toThrow("duplicatePath");
  });
  it("reads every native directory batch and preserves empty folders", async () => {
    const child = (name: string) => ({
      isFile: true,
      isDirectory: false,
      name,
      file: (done: (file: File) => void) => done(new File([name], name)),
    });
    const directory = (name: string, batches: unknown[][]) => ({
      isFile: false,
      isDirectory: true,
      name,
      createReader: () => ({
        readEntries: (done: (entries: unknown[]) => void) => done(batches.shift() || []),
      }),
    });
    const root = directory("tree", [[child("one"), directory("empty", [])], [child("two")]]);
    const transfer = {
      items: [{ kind: "file", webkitGetAsEntry: () => root, getAsFile: () => null }],
    } as unknown as DataTransfer;
    expect((await droppedEntries(transfer)).map((entry) => entry.path)).toEqual([
      "tree",
      "tree/one",
      "tree/empty",
      "tree/two",
    ]);
  });
  it("refuses overlarge batches and unsupported zero-byte directory placeholders", async () => {
    expect(() =>
      validateUploadEntries(Array.from({ length: 1001 }, (_, i) => ({ path: String(i) }))),
    ).toThrow("uploadLimit");
    expect(() =>
      validateUploadEntries([{ path: "large", file: { size: 101 * 1024 * 1024 } as File }]),
    ).toThrow("uploadLimit");
    await expect(
      droppedEntries({
        items: [{ kind: "file", getAsFile: () => new File([], "folder") }],
      } as unknown as DataTransfer),
    ).rejects.toThrow("unsupportedDrop");
  });
});
