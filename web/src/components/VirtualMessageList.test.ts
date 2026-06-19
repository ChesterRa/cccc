import { describe, expect, it } from "vitest";

import type { LedgerEvent } from "../types";
import { buildFederationDisplayNameMap } from "./virtualMessageListFederation";

describe("buildFederationDisplayNameMap", () => {
  it("maps federation chat senders to the remote group name from message metadata", () => {
    const messages: LedgerEvent[] = [
      {
        kind: "chat.message",
        by: "federation:peer_remote",
        data: { source_user_name: "Remote Group" },
      },
    ];

    expect(buildFederationDisplayNameMap(messages).get("federation:peer_remote")).toBe("Remote Group");
  });

  it("ignores non-chat events and federation messages without a source name", () => {
    const messages: LedgerEvent[] = [
      {
        kind: "chat.read",
        by: "federation:peer_read",
        data: { source_user_name: "Read Group" },
      },
      {
        kind: "chat.message",
        by: "federation:peer_without_name",
        data: { text: "hello" },
      },
    ];

    expect(buildFederationDisplayNameMap(messages).size).toBe(0);
  });
});
