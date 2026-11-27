// @vitest-environment happy-dom
import type { TFunction } from "i18next";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { downloadVoiceDocument } from "./downloadVoiceDocument";

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it.each([
  { title: "财务对账系统", path: "voice/f49208cd39524264.md", name: "财务对账系统.md" },
  { title: "Q4 Reconciliation Notes.md", path: "voice/id.md", name: "Q4 Reconciliation Notes.md" },
  { title: "财务/对账:系统?", path: "voice/id.md", name: "财务_对账_系统_.md" },
  { title: "", path: "voice/legacy.md", name: "legacy.md" },
  { title: "", path: "", name: "voice-secretary-document.md" },
])("downloads $name with the current document content", async ({ title, path, name }) => {
  vi.useFakeTimers();
  const createUrl = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:document-download");
  const revokeUrl = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const click = vi
    .spyOn(HTMLAnchorElement.prototype, "click")
    .mockImplementation(function (this: HTMLAnchorElement) {
      expect(this.isConnected).toBe(true);
    });
  const showNotice = vi.fn();
  const t = ((_key: string, options: { fileName: string }) =>
    `Downloaded ${options.fileName}.`) as unknown as TFunction;
  const content = "# 对账系统\n\n当前未保存的内容\n";

  downloadVoiceDocument(
    { document_id: "fixture", title, document_path: path, status: "active" },
    title,
    content,
    showNotice,
    t,
  );

  expect(click).toHaveBeenCalledOnce();
  const clickedAnchor = click.mock.contexts[0];
  expect(clickedAnchor?.download).toBe(name);
  expect(clickedAnchor?.href).toBe("blob:document-download");
  expect(clickedAnchor?.isConnected).toBe(false);
  const blob = createUrl.mock.calls[0][0] as Blob;
  expect(blob.type).toBe("text/markdown;charset=utf-8");
  expect(await blob.text()).toBe(content);
  expect(showNotice).toHaveBeenCalledWith({ message: `Downloaded ${name}.` });
  vi.runOnlyPendingTimers();
  expect(revokeUrl).toHaveBeenCalledWith("blob:document-download");
});
