import type { AssistantVoiceDocument } from "../../types";
import { apiJson } from "./base";
import { clearAssistantStateRequest } from "./groups";

export type VoiceFolder = { folder_id: string; name: string };
export type LibraryDocument = AssistantVoiceDocument & { folder_id?: string };
export type VoiceLibrary = { folders: VoiceFolder[]; documents: LibraryDocument[] };
export type LibraryAction = {
  action: "create_folder" | "rename_folder" | "remove_folder" | "move" | "restore" | "rename";
  folder_id?: string;
  name?: string;
  document_path?: string;
};
export async function voiceDocumentLibrary(groupId: string, action?: LibraryAction) {
  const result = await apiJson<VoiceLibrary>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/assistants/voice_secretary/documents/library`,
    action ? { method: "POST", body: JSON.stringify(action) } : undefined,
  );
  if (action) clearAssistantStateRequest(groupId);
  return result;
}
