import { describe, expect, it } from "vite-plus/test";

import { projectCrossGroupRecipients, projectMessageMode } from "./crossGroupRecipients";

describe("projectCrossGroupRecipients", () => {
  it("prefers the destination snapshot over canonical recipients", () => {
    expect(projectCrossGroupRecipients({ dst_to: ["peer"], to: ["@foreman"] })).toEqual(["peer"]);
  });

  it("reads canonical recipients from the real Rust ledger shape", () => {
    expect(projectCrossGroupRecipients({ to: ["@foreman"] })).toEqual(["@foreman"]);
  });

  it("defaults missing or blank recipients to foreman, never all", () => {
    expect(projectCrossGroupRecipients({ dst_to: ["  "], to: [] })).toEqual(["@foreman"]);
    expect(projectCrossGroupRecipients(undefined)).toEqual(["@foreman"]);
  });

  it("projects the remote delivery mode without changing local audit semantics", () => {
    expect(
      projectMessageMode({
        dst_group_id: "g-remote",
        dst_message_mode: "mail",
        message_mode: "send",
      }),
    ).toBe("mail");
    expect(projectMessageMode({ message_mode: "request_reply" })).toBe("request_reply");
  });
});
