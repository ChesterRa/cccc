import { apiJson, asRecord, clearGroupsReadRequest, type ApiResponse } from "./base";

type GroupCreationResult = { group_id: string };

export async function createGroup(
  title: string,
  topic: string = "",
): Promise<ApiResponse<GroupCreationResult>> {
  return create({ title, topic });
}

export async function createGroupWithScope(
  title: string,
  path: string,
  topic: string = "",
): Promise<ApiResponse<GroupCreationResult>> {
  return create({ title, topic, path });
}

async function create(input: {
  title: string;
  topic: string;
  path?: string;
}): Promise<ApiResponse<GroupCreationResult>> {
  clearGroupsReadRequest();
  const response = await apiJson<unknown>("/api/v1/groups", {
    method: "POST",
    body: JSON.stringify({ ...input, by: "user" }),
  });
  if (!response.ok) return response;
  const result = asRecord(response.result);
  const group = asRecord(result?.group);
  const top = strictId(result, "group_id");
  const nested = strictId(group, "group_id");
  if (top === null || nested === null || (!top && !nested) || (top && nested && top !== nested)) {
    return {
      ok: false,
      error: { code: "invalid_response", message: "Invalid group create response" },
    };
  }
  return { ok: true, result: { group_id: top || nested || "" } };
}

function strictId(record: Record<string, unknown> | null, key: string): string | null {
  if (!record || !Object.prototype.hasOwnProperty.call(record, key)) return "";
  const value = record[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
