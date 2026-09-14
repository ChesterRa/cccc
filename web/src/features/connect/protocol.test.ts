// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { buildTerminalWebSocketUrl } from "../../utils/terminalConnection";
import { withAuthToken } from "../../services/api/base";
import { getPresentationBrowserSurfaceWebSocketUrl } from "../../services/api/groups";
import {
  acceptsFrameMessage,
  CONNECT_CHANNEL,
  endConnectFrame,
  frameResourceUrl,
  readFrameProof,
  remoteGroups,
} from "./protocol";

const proof = {
  frame_id: "frame-1",
  target_instance_id: "B",
  target_device_id: "device-b",
  parent_origin: "http://entry.test",
  expires_at: "2099-01-01T00:00:00Z",
  signature: "verified-by-server",
};
const query = `?proof=${btoa(JSON.stringify(proof)).replace(/=+$/, "")}`;

describe("Connect frame routing", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("never treats an ordinary workbench query as an embedded session", () => {
    expect(readFrameProof({ pathname: "/ui/", search: query })).toBeNull();
    expect(readFrameProof({ pathname: "/ui/connect/", search: query })).toEqual(proof);
    expect(readFrameProof({ pathname: "/ui/connect/", search: "?proof=invalid" })).toBeNull();
  });
  it("requires exact origin, window and frame correlation for every parent message", () => {
    const event = new MessageEvent("message", {
      origin: "https://b.test",
      source: window,
      data: { channel: CONNECT_CHANNEL, frame_id: "one" },
    });
    expect(acceptsFrameMessage(event, window, "https://b.test", "one")).toBe(true);
    expect(acceptsFrameMessage(event, null, "https://b.test", "one")).toBe(false);
    expect(acceptsFrameMessage(event, window, "https://c.test", "one")).toBe(false);
    expect(acceptsFrameMessage(event, window, "https://b.test", "two")).toBe(false);
  });
  it("only adds the admitted frame id to target-owned Group resources", () => {
    const location = new URL(`https://b.test/ui/connect/${query}`) as unknown as Location;
    expect(frameResourceUrl("/api/v1/groups/g/presentation/slots/slot-1/asset?v=2", location)).toBe(
      "/api/v1/groups/g/presentation/slots/slot-1/asset?v=2&connect_frame=frame-1",
    );
    expect(frameResourceUrl("https://c.test/api/v1/groups/g/blobs/file", location)).toBe(
      "https://c.test/api/v1/groups/g/blobs/file",
    );
    expect(frameResourceUrl("/api/v1/settings", location)).toBe("/api/v1/settings");
    expect(frameResourceUrl("blob:https://b.test/one", location)).toBe("blob:https://b.test/one");
  });
  it("accepts minimal group metadata and discards other target fields", () => {
    expect(
      remoteGroups([{ group_id: "g", title: "B Group", running: true, token: "discard" }]),
    ).toEqual([{ group_id: "g", title: "B Group", running: true }]);
    expect(remoteGroups([{ group_id: "g", title: "B", running: "true" }])).toBeNull();
    expect(remoteGroups([null])).toBeNull();
  });
  it.each(["https://b.test", "https://b.test:9443", "http://b.test:8080"])(
    "carries frame authority through the real stream URL builders at %s",
    (origin) => {
      const location = new URL(`${origin}/ui/connect/${query}`);
      vi.stubGlobal("window", { location });
      const terminal = new URL(
        withAuthToken(
          buildTerminalWebSocketUrl({
            protocol: location.protocol,
            host: location.host,
            groupId: "g",
            actorId: "worker",
            mode: "viewer",
            since: 42,
          }),
        ),
      );
      expect(terminal.searchParams.get("connect_frame")).toBe("frame-1");
      expect(terminal.searchParams.get("mode")).toBe("viewer");
      expect(terminal.searchParams.get("since")).toBe("42");
      const presentation = new URL(getPresentationBrowserSurfaceWebSocketUrl("g", "slot-1"));
      expect(presentation.searchParams.get("connect_frame")).toBe("frame-1");
      expect(presentation.searchParams.get("slot")).toBe("slot-1");
      expect(withAuthToken("/api/v1/events/stream")).toBe(
        "/api/v1/events/stream?connect_frame=frame-1",
      );
    },
  );
  it.each([
    "wss://c.test/api/v1/groups/g/actors/worker/term",
    "wss://b.test:9443/api/v1/groups/g/actors/worker/term",
    "ws://b.test/api/v1/groups/g/actors/worker/term",
    "https://c.test/api/v1/events/stream",
    "https://b.test/api/v1/events/stream/other",
  ])("does not attach frame authority to a different origin or resource: %s", (url) => {
    const location = new URL(`https://b.test/ui/connect/${query}`) as unknown as Location;
    expect(frameResourceUrl(url, location)).toBe(url);
  });
  it("returns logout and listener replacement to the exact entry without replaying the page proof", () => {
    const postMessage = vi.fn();
    vi.stubGlobal("window", {
      location: { pathname: "/ui/connect/", search: query },
      parent: { postMessage },
    });
    expect(endConnectFrame()).toBe(true);
    expect(postMessage).toHaveBeenCalledWith(
      { channel: CONNECT_CHANNEL, frame_id: proof.frame_id, type: "expired" },
      proof.parent_origin,
    );
    window.location.pathname = "/ui/";
    expect(endConnectFrame()).toBe(false);
    expect(postMessage).toHaveBeenCalledTimes(1);
  });
});
