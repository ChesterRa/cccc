import { describe, expect, it } from "vite-plus/test";

import { actorSecretDraftReducer, emptyActorSecretDraftState } from "./actorSecretDraftModel";

describe("actorSecretDraftModel", () => {
  it("discards the plaintext edit draft when its key is removed", () => {
    let state = actorSecretDraftReducer(emptyActorSecretDraftState(), {
      type: "startEdit",
      key: "OPENAI_API_KEY",
    });
    state = actorSecretDraftReducer(state, { type: "setEditValue", value: "plaintext-secret" });
    state = actorSecretDraftReducer(state, { type: "toggleEditVisibility" });

    expect(actorSecretDraftReducer(state, { type: "discardKey", key: "OPENAI_API_KEY" })).toEqual(
      emptyActorSecretDraftState(),
    );
  });

  it("discards add and edit plaintext drafts when clear all is staged", () => {
    let state = actorSecretDraftReducer(emptyActorSecretDraftState(), { type: "openAdd" });
    state = actorSecretDraftReducer(state, { type: "setAddKey", value: "NEW_KEY" });
    state = actorSecretDraftReducer(state, { type: "setAddValue", value: "new-secret" });
    state = actorSecretDraftReducer(state, { type: "startEdit", key: "OLD_KEY" });
    state = actorSecretDraftReducer(state, { type: "setEditValue", value: "replacement-secret" });

    expect(actorSecretDraftReducer(state, { type: "discardAll" })).toEqual(
      emptyActorSecretDraftState(),
    );
  });

  it("does not discard another key's edit draft", () => {
    let state = actorSecretDraftReducer(emptyActorSecretDraftState(), {
      type: "startEdit",
      key: "KEEP_KEY",
    });
    state = actorSecretDraftReducer(state, { type: "setEditValue", value: "replacement-secret" });

    expect(actorSecretDraftReducer(state, { type: "discardKey", key: "OTHER_KEY" })).toEqual(state);
  });

  it("discards a matching add-form plaintext draft when an existing key is removed", () => {
    let state = actorSecretDraftReducer(emptyActorSecretDraftState(), { type: "openAdd" });
    state = actorSecretDraftReducer(state, { type: "setAddKey", value: " OPENAI_API_KEY " });
    state = actorSecretDraftReducer(state, { type: "setAddValue", value: "replacement-secret" });

    expect(actorSecretDraftReducer(state, { type: "discardKey", key: "OPENAI_API_KEY" })).toEqual(
      emptyActorSecretDraftState(),
    );
  });
});
