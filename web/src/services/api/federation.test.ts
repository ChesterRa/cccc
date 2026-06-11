import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildFederationTargetBody,
  fetchFederationDeliveryStatus,
  fetchFederationStatus,
  registerFederation,
  unregisterFederation,
  verifyFederation,
} from "./federation";

function okResponse(result: unknown): Response {
  return new Response(JSON.stringify({ ok: true, result }));
}

describe("federation API client", () => {
  const credentialRef = "sec_remote_peer";

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("builds the target body with credential in the body payload", () => {
    const body = buildFederationTargetBody({
      groupId: "g1",
      url: "https://hub/",
      credentialRef,
      remoteGroupId: "g_r",
    });
    expect(body).toEqual({
      group_id: "g1",
      url: "https://hub/",
      transport: "peer_cccc_http",
      remote_group_id: "g_r",
      credential_ref: credentialRef,
    });
  });

  it("verify posts to the verify route", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      okResponse({ verified: true, group_id: "g1", normalized_url: "https://hub", transport: "peer_cccc_http" }),
    );
    await verifyFederation({ groupId: "g1", url: "https://hub/", credentialRef });
    const [url, init] = fetchMock.mock.calls[0] || [];
    expect(String(url)).toBe("/api/federation/verify");
    expect(String((init as RequestInit)?.method)).toBe("POST");
    const sent = JSON.parse(String((init as RequestInit)?.body));
    expect(sent.credential_ref).toBe(credentialRef);
    expect(sent.group_id).toBe("g1");
  });

  it("register posts to the register route with credential in body", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      okResponse({ registration: { registration_id: "r1", group_id: "g1" } }),
    );
    await registerFederation({ groupId: "g1", url: "https://hub/", credentialRef });
    const [url, init] = fetchMock.mock.calls[0] || [];
    expect(String(url)).toBe("/api/federation/register");
    const sent = JSON.parse(String((init as RequestInit)?.body));
    expect(sent.credential_ref).toBe(credentialRef);
  });

  it("status / delivery-status use the expected routes", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({ registrations: [] }));
    await fetchFederationStatus("g1");
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe("/api/federation/status?group_id=g1");

    await fetchFederationDeliveryStatus("r1", "k1");
    expect(String(fetchMock.mock.calls[1]?.[0])).toBe("/api/federation/registrations/r1/deliveries/k1");

    await unregisterFederation("r1");
    const [url, init] = fetchMock.mock.calls[2] || [];
    expect(String(url)).toBe("/api/federation/unregister");
    expect(JSON.parse(String((init as RequestInit)?.body)).registration_id).toBe("r1");
  });

  it("never writes the credential to web storage", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const setItem = vi.fn();
    const storage = { getItem: vi.fn().mockReturnValue(null), setItem, removeItem: vi.fn() };
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("sessionStorage", storage);
    vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({ registration: { registration_id: "r1" } }));

    await registerFederation({ groupId: "g1", url: "https://hub/", credentialRef });
    await verifyFederation({ groupId: "g1", url: "https://hub/", credentialRef });

    expect(setItem).not.toHaveBeenCalled();
  });
});
