import type { AssistantVoiceDocumentMutationResult } from "../../types";
import { apiJson } from "./base";
import { clearAssistantStateRequest } from "./groups";

export async function deleteVoiceAssistantDocument(groupId: string, documentPath: string) {
  clearAssistantStateRequest(groupId);
  const response = await apiJson<AssistantVoiceDocumentMutationResult>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/assistants/voice_secretary/documents/delete`,
    { method: "POST", body: JSON.stringify({ document_path: documentPath }) },
  );
  clearAssistantStateRequest(groupId);
  return response;
}
