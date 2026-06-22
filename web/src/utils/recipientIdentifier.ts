export type RecipientIdentifierInput =
  | {
      kind: "selector";
      selector?: string;
    }
  | {
      kind: "actor";
      label?: string;
      id?: string;
      role?: string;
    }
  | {
      kind: "remote_group";
      label?: string;
      id?: string;
      accessLevel?: string;
    };

function cleanValue(value: unknown): string {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function cleanRole(value: unknown): string {
  return cleanValue(value).toLowerCase();
}

function remoteAccessLabel(value: unknown): string {
  void value;
  return "message only";
}

export function formatRecipientIdentifier(input: RecipientIdentifierInput): string {
  if (input.kind === "selector") {
    const selector = cleanValue(input.selector) || "@all";
    return `${selector} (local selector)`;
  }

  if (input.kind === "actor") {
    const id = cleanValue(input.id);
    const label = cleanValue(input.label) || id || "actor";
    const role = cleanRole(input.role);
    const scope = role ? `local/${role}` : "local actor";
    if (id && id !== label) return `${label} (${id} ${scope})`;
    return `${label} (${scope})`;
  }

  const id = cleanValue(input.id) || "<remote_group_id>";
  const label = cleanValue(input.label) || id || "remote group";
  return `${label} (${id} remote/${remoteAccessLabel(input.accessLevel)})`;
}
