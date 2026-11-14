import { apiJson, type ApiResponse } from "../../services/api/base";
import {
  sameDirectListener,
  readDirectInvitation,
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
  expiresInSeconds = 604800,
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
  if (!initial.result.capabilities?.reusable_invitations) return fail("direct_upgrade_required");
  if (expiresInSeconds === 0 && !initial.result.capabilities?.permanent_invitations)
    return fail("direct_permanent_upgrade_required");
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
    if (!check.result.capabilities?.reusable_invitations) return fail("direct_upgrade_required");
    if (expiresInSeconds === 0 && !check.result.capabilities?.permanent_invitations)
      return fail("direct_permanent_upgrade_required");
    if (!sameDirectListener(check.result.listener, listener))
      return fail("direct_settings_changed");
    if (check.result.runtime?.error) return fail("direct_listener_failed");
    if (check.result.runtime?.listener) {
      if (signal.aborted) break;
      const invited = await post({
        action: "invite",
        expected_listener: listener,
        reusable: true,
        expires_in_seconds: expiresInSeconds,
      });
      if (!invited.ok) return invited;
      // A server replaced between the status read and POST must not silently
      // turn a reusable invitation into a legacy single-use one.
      const preview = invited.result.invitation
        ? readDirectInvitation(invited.result.invitation)
        : null;
      if (!preview?.reusable || !!preview.permanent !== (expiresInSeconds === 0))
        return fail("direct_upgrade_required");
      return invited;
    }
    if (Date.now() >= deadline) return fail("direct_listener_timeout");
    await waitForCheck(signal);
  }
  return fail("direct_action_aborted");
}
