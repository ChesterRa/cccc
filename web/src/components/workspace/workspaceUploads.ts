/** Browser-native folder traversal, bounded before any server-side changes. */
export const WORKSPACE_DRAG_TYPE = "application/x-cccc-workspace-entry";
export const MAX_UPLOAD_ENTRIES = 1000;
export const MAX_UPLOAD_BYTES = 100 * 1024 * 1024;
export type UploadEntry = { path: string; file?: File };
export const joinWorkspacePath = (parent: string, name: string) =>
  parent ? `${parent}/${name}` : name;

export function validateUploadEntries(entries: UploadEntry[]): UploadEntry[] {
  const paths = new Set<string>();
  let bytes = 0;
  for (const entry of entries) {
    const parts = entry.path.split("/");
    if (
      parts.some(
        (part) =>
          !part ||
          part === "." ||
          part === ".." ||
          part.toLowerCase() === ".git" ||
          part.includes("\0"),
      )
    )
      throw new Error("invalidPath");
    if (paths.has(entry.path)) throw new Error("duplicatePath");
    paths.add(entry.path);
    bytes += entry.file?.size ?? 0;
    if (paths.size > MAX_UPLOAD_ENTRIES || bytes > MAX_UPLOAD_BYTES) throw new Error("uploadLimit");
  }
  return entries;
}

export function pickerEntries(files: File[]): UploadEntry[] {
  const directories = new Set<string>();
  const entries: UploadEntry[] = [];
  for (const file of files) {
    const path = file.webkitRelativePath || file.name;
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) {
      const directory = parts.slice(0, i).join("/");
      if (!directories.has(directory)) {
        directories.add(directory);
        entries.push({ path: directory });
      }
    }
    entries.push({ path, file });
  }
  return validateUploadEntries(entries);
}

// Capture the entries synchronously: browsers protect the DataTransfer after drop dispatch.
export function droppedEntries(transfer: DataTransfer): Promise<UploadEntry[]> {
  const items = Array.from(transfer.items).filter((item) => item.kind === "file");
  const roots = items.map((item) => ({ entry: item.webkitGetAsEntry?.(), file: item.getAsFile() }));
  return (async () => {
    const result: UploadEntry[] = [];
    let bytes = 0;
    const add = (entry: UploadEntry) => {
      result.push(entry);
      bytes += entry.file?.size ?? 0;
      if (result.length > MAX_UPLOAD_ENTRIES || bytes > MAX_UPLOAD_BYTES)
        throw new Error("uploadLimit");
    };
    const walk = async (entry: FileSystemEntry, parent: string): Promise<void> => {
      const path = joinWorkspacePath(parent, entry.name);
      if (entry.isFile) {
        const file = await new Promise<File>((resolve, reject) =>
          (entry as FileSystemFileEntry).file(resolve, reject),
        );
        add({ path, file });
      } else if (entry.isDirectory) {
        add({ path });
        const reader = (entry as FileSystemDirectoryEntry).createReader();
        for (;;) {
          const children = await new Promise<FileSystemEntry[]>((resolve, reject) =>
            reader.readEntries(resolve, reject),
          );
          if (!children.length) break;
          for (const child of children) await walk(child, path);
        }
      } else throw new Error("unsupportedDrop");
    };
    if (!roots.length) throw new Error("unsupportedDrop");
    for (const root of roots) {
      if (root.entry) await walk(root.entry, "");
      else if (root.file && root.file.size > 0) add({ path: root.file.name, file: root.file });
      else throw new Error("unsupportedDrop");
    }
    return validateUploadEntries(result);
  })();
}
