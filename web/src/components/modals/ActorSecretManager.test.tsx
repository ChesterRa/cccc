import { createInstance } from "i18next";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vite-plus/test";

import actorsEn from "../../i18n/locales/en/actors.json";
import { ActorConfigModal } from "./ActorConfigModal";
import { ActorSecretManager } from "./ActorSecretManager";
import { emptyActorSecretChanges, setActorSecretClearAll } from "./actorSecretManagerModel";
import { SecretValueInput } from "./SecretValueInput";

const i18n = createInstance();

beforeAll(async () => {
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "actors",
      resources: { en: { actors: actorsEn } },
      interpolation: { escapeValue: false },
    });
});

function renderManager(
  clearAll = false,
  loading = false,
  keysLoadFailed = false,
  keys = ["OPENAI_API_KEY", "A_VERY_LONG_ENVIRONMENT_VARIABLE_NAME"],
) {
  const changes = setActorSecretClearAll(emptyActorSecretChanges(), clearAll);
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <ActorSecretManager
        keys={keys}
        masks={{ OPENAI_API_KEY: "sk-******1234" }}
        changes={changes}
        loading={loading}
        keysLoadFailed={keysLoadFailed}
        onRefresh={() => undefined}
        onChangesChange={() => undefined}
      />
    </I18nextProvider>,
  );
}

function clearAllButton(markup: string) {
  return [...markup.matchAll(/<button\b[^>]*>.*?<\/button>/gs)]
    .map(([button]) => button)
    .find((button) => button.includes("Clear all</button>"));
}

describe("ActorSecretManager", () => {
  it("renders configured variables and direct management actions", () => {
    const markup = renderManager();

    expect(markup).toContain("Environment variables");
    expect(markup).not.toContain(">Environment variables</div>");
    expect(markup).not.toContain(
      "Add, update, or remove variables here. Changes apply when you save.",
    );
    expect(markup).toContain("OPENAI_API_KEY");
    expect(markup).toContain("A_VERY_LONG_ENVIRONMENT_VARIABLE_NAME");
    expect(markup).toContain('aria-label="Update OPENAI_API_KEY"');
    expect(markup).toContain('aria-label="Remove OPENAI_API_KEY"');
    expect(markup).toContain("Add variable");
    expect(markup).toContain("Batch paste");
    expect(markup).toContain('aria-label="Refresh configured keys"');
    expect(markup.indexOf("Batch paste")).toBeLessThan(markup.indexOf("Add variable"));
    expect(markup).toContain("Clear all variables");
  });

  it("replaces mutation controls with an undoable clear-all warning", () => {
    const markup = renderManager(true);

    expect(markup).toContain("All variables will be cleared when you save.");
    expect(markup).toContain("Undo");
    expect(markup).toContain('aria-label="Refresh configured keys"');
    expect(markup).not.toContain('aria-label="Update OPENAI_API_KEY"');
    expect(markup).not.toContain('aria-label="Remove OPENAI_API_KEY"');
  });

  it("shows a loading state instead of an empty-state flash and disables actions", () => {
    const markup = renderManager(false, true);

    expect(markup).toContain("Loading variables…");
    expect(markup).not.toContain("No variables configured.");
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*>.*Add variable/s);
  });

  it("disables clearing when the key list failed to load", () => {
    const markup = renderManager(false, false, true, []);

    expect(clearAllButton(markup)).toContain('disabled=""');
  });

  it("disables clearing when a successfully loaded key list is empty", () => {
    const markup = renderManager(false, false, false, []);

    expect(clearAllButton(markup)).toContain('disabled=""');
  });
});

describe("SecretValueInput", () => {
  it("gives the password field and visibility control distinct accessible names", () => {
    const markup = renderToStaticMarkup(
      <SecretValueInput
        id="new-secret-value"
        ariaLabel="New value for OPENAI_API_KEY"
        value=""
        visible={false}
        disabled={false}
        placeholder="Enter new value"
        showLabel="Show value"
        hideLabel="Hide value"
        onChange={() => undefined}
        onToggleVisibility={() => undefined}
      />,
    );

    expect(markup).toContain('aria-label="New value for OPENAI_API_KEY"');
    expect(markup).toContain('aria-label="Show value"');
  });
});

describe("ActorConfigModal secret manager integration", () => {
  it("renders the structured manager instead of command textareas in edit mode", () => {
    const markup = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <ActorConfigModal
          mode="edit"
          isOpen
          isDark={false}
          busy=""
          groupId="group-1"
          actorId="actor-1"
          isRunning={false}
          runtimes={[]}
          runtime="codex"
          onChangeRuntime={() => undefined}
          runner="pty"
          onChangeRunner={() => undefined}
          command="codex"
          onChangeCommand={() => undefined}
          title=""
          onChangeTitle={() => undefined}
          actorNotes=""
          onChangeActorNotes={() => undefined}
          capabilityAutoloadText=""
          onChangeCapabilityAutoloadText={() => undefined}
          actorProfiles={[]}
          actorProfilesBusy={false}
          onSaveAsProfile={() => undefined}
          onSave={async () => undefined}
          onSaveAndRestart={async () => undefined}
          onCancel={() => undefined}
        />
      </I18nextProvider>,
    );

    expect(markup).toContain("Add variable");
    expect(markup).not.toContain("Set / Update");
    expect(markup).not.toContain("Clear all secret keys on save");
    expect(markup).not.toContain("Secrets (write-only)");
    expect(markup).not.toContain("Stored locally under");
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('role="tab" aria-selected="true"');
    expect(markup).not.toContain("<summary");
  });

  it("uses the same always-visible advanced tabs in create mode", () => {
    const markup = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <ActorConfigModal
          mode="create"
          isOpen
          isDark={false}
          busy=""
          hasForeman={false}
          suggestedActorId="actor-1"
          actorId=""
          onChangeActorId={() => undefined}
          role="peer"
          onChangeRole={() => undefined}
          useProfile={false}
          onChangeUseProfile={() => undefined}
          profileId=""
          onChangeProfileId={() => undefined}
          runtimes={[]}
          runtime="codex"
          onChangeRuntime={() => undefined}
          runner="pty"
          onChangeRunner={() => undefined}
          command="codex"
          onChangeCommand={() => undefined}
          useDefaultCommand
          onChangeUseDefaultCommand={() => undefined}
          secretsSetText=""
          onChangeSecretsSetText={() => undefined}
          capabilityAutoloadText=""
          onChangeCapabilityAutoloadText={() => undefined}
          actorNotes=""
          onChangeActorNotes={() => undefined}
          actorProfiles={[]}
          actorProfilesBusy={false}
          onSaveAsProfile={() => undefined}
          error=""
          onChangeError={() => undefined}
          canSubmit
          submitDisabledReason=""
          onCreate={() => true}
          onCancel={() => undefined}
        />
      </I18nextProvider>,
    );

    expect(markup).toContain('role="tablist"');
    expect(markup).toContain("Environment variables");
    expect(markup).not.toContain("<summary");
  });
});
