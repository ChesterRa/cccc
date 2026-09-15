import { apiJson, type ApiResponse } from "../../services/api/base";
import {
  sameDirectListener,
  type DirectListener,
  type DirectStatus,
} from "./directConnectionModel";

const fail = (code: string): ApiResponse<never> => ({ ok: false, error: { code, message: code } });

function waitForCheck(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      clearTimeout(timer);
      signal.removeEventListener("abort", finish);
      resolve();
    };
    const timer = setTimeout(finish, 500);
    signal.addEventListener("abort", finish, { once: true });
    if (signal.aborted) finish();
  });
}

/** One explicit action. Never resume this workflow from a status poll or retry a POST. */
export async function prepareDirectInvitation(
  groupId: string,
  previous: DirectListener | null,
  listener: DirectListener,
  name: string,
  signal: AbortSignal,
): Promise<ApiResponse<{ invitation?: string }>> {
  const post = (body: Record<string, unknown>) =>
    apiJson<{ invitation?: string }>("/api/v1/connect/direct", {
      method: "POST",
      signal,
      body: JSON.stringify({ group_id: groupId, ...body }),
    });
  const statusUrl = `/api/v1/connect/direct?group_id=${encodeURIComponent(groupId)}`;
  const initial = await apiJson<DirectStatus>(statusUrl, { signal });
  if (!initial.ok) return initial;
  if (signal.aborted) return fail("direct_action_aborted");
  const current = initial.result.listener;
  if (!sameDirectListener(current, previous) && !sameDirectListener(current, listener))
    return fail("direct_settings_changed");
  if (!sameDirectListener(current, listener) || initial.result.display_name !== name) {
    const configured = await post({
      action: "configure",
      listener,
      display_name: name,
      expected_listener: current,
    });
    if (!configured.ok) return configured;
  }
  const deadline = Date.now() + 10000;
  while (!signal.aborted) {
    const check = await apiJson<DirectStatus>(statusUrl, { signal });
    if (!check.ok) return check;
    if (!sameDirectListener(check.result.listener, listener))
      return fail("direct_settings_changed");
    if (check.result.runtime?.error) return fail("direct_listener_failed");
    if (check.result.runtime?.listener) {
      if (signal.aborted) break;
      return post({ action: "invite", expected_listener: listener });
    }
    if (Date.now() >= deadline) return fail("direct_listener_timeout");
    await waitForCheck(signal);
  }
  return fail("direct_action_aborted");
}
