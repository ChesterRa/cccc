export type HelpChangedBlock = "common" | "role:foreman" | "role:peer" | "voice_secretary" | `actor:${string}`;

export type ParsedHelpMarkdown = {
  common: string;
  foreman: string;
  peer: string;
  voiceSecretary: string;
  actorNotes: Record<string, string>;
  extraTaggedBlocks: string[];
  usedLegacyRoleNotes: boolean;
};

type TaggedSection = {
  kind: "role" | "actor" | "voice_secretary" | "extra";
  key: string;
  raw: string;
  body: string;
};

const H2_RE = /^##(?!#)\s+.*$/;
const ROLE_TAG_RE = /^##\s*@role:\s*(\w+)\s*$/i;
const ACTOR_TAG_RE = /^##\s*@actor:\s*(\S+)(?:\s+(.*\S))?\s*$/i;
const PET_TAG_RE = /^##\s*@pet\s*:?\s*$/i;
const VOICE_SECRETARY_TAG_RE = /^##\s*@voice_secretary\s*:?\s*$/i;
const LEGACY_ROLE_SECTION_RE = /^##\s+Role Notes\s*$/i;
const H3_RE = /^###\s+(.+?)\s*$/;

function splitSections(markdown: string): string[] {
  const raw = String(markdown || "").replace(/\r\n?/g, "\n");
  if (!raw) return [""];
  const lines = raw.split("\n");
  const sections: string[] = [];
  let current: string[] = [];
  for (const line of lines) {
    if (H2_RE.test(line) && current.length > 0) {
      sections.push(current.join("\n"));
      current = [line];
      continue;
    }
    current.push(line);
  }
  sections.push(current.join("\n"));
  return sections;
}

function trimBlock(text: string): string {
  return String(text || "").trim();
}

function parseTaggedSection(section: string): TaggedSection | null {
  const normalized = String(section || "").replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n");
  const header = String(lines[0] || "");
  const roleMatch = header.match(ROLE_TAG_RE);
  if (roleMatch) {
    const role = String(roleMatch[1] || "").trim().toLowerCase();
    const body = trimBlock(lines.slice(1).join("\n"));
    if (role === "foreman" || role === "peer") {
      return { kind: "role", key: `role:${role}`, raw: trimBlock(normalized), body };
    }
    return { kind: "extra", key: `role:${role}`, raw: trimBlock(normalized), body };
  }
  const actorMatch = header.match(ACTOR_TAG_RE);
  if (actorMatch) {
    const actorId = String(actorMatch[1] || "").trim();
    const inlineBody = String(actorMatch[2] || "").trim();
    const bodyLines = lines.slice(1);
    const body = trimBlock([inlineBody, ...bodyLines].filter(Boolean).join("\n"));
    return {
      kind: actorId ? "actor" : "extra",
      key: actorId ? `actor:${actorId}` : "actor:",
      raw: trimBlock(normalized),
      body,
    };
  }
  if (PET_TAG_RE.test(header)) {
    return {
      kind: "extra",
      key: "legacy:pet",
      raw: trimBlock(normalized),
      body: trimBlock(lines.slice(1).join("\n")),
    };
  }
  if (VOICE_SECRETARY_TAG_RE.test(header)) {
    return {
      kind: "voice_secretary",
      key: "voice_secretary",
      raw: trimBlock(normalized),
      body: trimBlock(lines.slice(1).join("\n")),
    };
  }
  return null;
}

function tryExtractLegacyRoleNotes(common: string): {
  common: string;
  foreman: string;
  peer: string;
  used: boolean;
} {
  const normalized = String(common || "").replace(/\r\n?/g, "\n");
  const sections = splitSections(normalized);
  const kept: string[] = [];
  let foreman = "";
  let peer = "";
  let used = false;

  for (const section of sections) {
    const raw = trimBlock(section);
    if (!raw) continue;
    const lines = raw.split("\n");
    if (!LEGACY_ROLE_SECTION_RE.test(String(lines[0] || ""))) {
      kept.push(raw);
      continue;
    }
    const bodyLines = lines.slice(1);
    const chunks: Array<{ title: string; body: string }> = [];
    let currentTitle = "";
    let currentBody: string[] = [];
    let stray = false;
    for (const line of bodyLines) {
      const h3 = line.match(H3_RE);
      if (h3) {
        if (currentTitle) {
          chunks.push({ title: currentTitle, body: trimBlock(currentBody.join("\n")) });
        } else if (trimBlock(currentBody.join("\n"))) {
          stray = true;
        }
        currentTitle = String(h3[1] || "").trim();
        currentBody = [];
        continue;
      }
      currentBody.push(line);
    }
    if (currentTitle) {
      chunks.push({ title: currentTitle, body: trimBlock(currentBody.join("\n")) });
    } else if (trimBlock(currentBody.join("\n"))) {
      stray = true;
    }
    if (stray || !chunks.length) {
      kept.push(raw);
      continue;
    }
    let localForeman = "";
    let localPeer = "";
    let unknown = false;
    for (const chunk of chunks) {
      const title = chunk.title.trim().toLowerCase();
      if (title === "foreman") {
        localForeman = chunk.body;
      } else if (title === "peer") {
        localPeer = chunk.body;
      } else {
        unknown = true;
        break;
      }
    }
    if (unknown) {
      kept.push(raw);
      continue;
    }
    foreman = localForeman;
    peer = localPeer;
    used = true;
  }

  return {
    common: kept.join("\n\n").trim(),
    foreman,
    peer,
    used,
  };
}

export function parseHelpMarkdown(markdown: string): ParsedHelpMarkdown {
  const sections = splitSections(markdown);
  const commonSections: string[] = [];
  const actorNotes: Record<string, string> = {};
  const extraTaggedBlocks: string[] = [];
  let foreman = "";
  let peer = "";
  let voiceSecretary = "";

  for (const section of sections) {
    const raw = trimBlock(section);
    if (!raw) continue;
    const tagged = parseTaggedSection(raw);
    if (!tagged) {
      commonSections.push(raw);
      continue;
    }
    if (tagged.kind === "role") {
      if (tagged.key === "role:foreman") foreman = tagged.body;
      else if (tagged.key === "role:peer") peer = tagged.body;
      else extraTaggedBlocks.push(tagged.raw);
      continue;
    }
    if (tagged.kind === "actor") {
      const actorId = tagged.key.slice("actor:".length);
      if (actorId) actorNotes[actorId] = tagged.body;
      else extraTaggedBlocks.push(tagged.raw);
      continue;
    }
    if (tagged.kind === "voice_secretary") {
      voiceSecretary = tagged.body;
      continue;
    }
    extraTaggedBlocks.push(tagged.raw);
  }

  let common = commonSections.join("\n\n").trim();
  let usedLegacyRoleNotes = false;
  if (!foreman && !peer) {
    const legacy = tryExtractLegacyRoleNotes(common);
    common = legacy.common;
    if (legacy.foreman) foreman = legacy.foreman;
    if (legacy.peer) peer = legacy.peer;
    usedLegacyRoleNotes = legacy.used;
  }

  return { common, foreman, peer, voiceSecretary, actorNotes, extraTaggedBlocks, usedLegacyRoleNotes };
}

export function buildHelpMarkdown(input: {
  common: string;
  foreman: string;
  peer: string;
  voiceSecretary?: string;
  actorNotes: Record<string, string>;
  actorOrder?: string[];
  extraTaggedBlocks?: string[];
}): string {
  const parts: string[] = [];
  const common = trimBlock(input.common);
  const foreman = trimBlock(input.foreman);
  const peer = trimBlock(input.peer);
  const voiceSecretary = trimBlock(input.voiceSecretary || "");
  const actorNotes = input.actorNotes || {};
  const extraTaggedBlocks = Array.isArray(input.extraTaggedBlocks) ? input.extraTaggedBlocks.map(trimBlock).filter(Boolean) : [];

  if (common) parts.push(common);
  if (foreman) parts.push(`## @role: foreman\n\n${foreman}`);
  if (peer) parts.push(`## @role: peer\n\n${peer}`);
  if (voiceSecretary) parts.push(`## @voice_secretary\n\n${voiceSecretary}`);

  const seen = new Set<string>();
  const orderedActorIds: string[] = [];
  for (const actorId of Array.isArray(input.actorOrder) ? input.actorOrder : []) {
    const id = String(actorId || "").trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    orderedActorIds.push(id);
  }
  for (const actorId of Object.keys(actorNotes).sort()) {
    if (seen.has(actorId)) continue;
    seen.add(actorId);
    orderedActorIds.push(actorId);
  }
  for (const actorId of orderedActorIds) {
    const body = trimBlock(actorNotes[actorId]);
    if (!body) continue;
    parts.push(`## @actor: ${actorId}\n\n${body}`);
  }
  parts.push(...extraTaggedBlocks);
  const out = parts.filter(Boolean).join("\n\n").trim();
  return out ? `${out}\n` : "";
}

export function updateActorHelpNote(markdown: string, actorId: string, note: string, actorOrder?: string[]): string {
  const parsed = parseHelpMarkdown(markdown);
  const nextActorNotes = { ...parsed.actorNotes };
  const aid = String(actorId || "").trim();
  if (aid) nextActorNotes[aid] = trimBlock(note);
  if (aid && !nextActorNotes[aid]) delete nextActorNotes[aid];
  return buildHelpMarkdown({
    common: parsed.common,
    foreman: parsed.foreman,
    peer: parsed.peer,
    voiceSecretary: parsed.voiceSecretary,
    actorNotes: nextActorNotes,
    actorOrder,
    extraTaggedBlocks: parsed.extraTaggedBlocks,
  });
}

export function updateVoiceSecretaryHelpNote(markdown: string, note: string, actorOrder?: string[]): string {
  const parsed = parseHelpMarkdown(markdown);
  return buildHelpMarkdown({
    common: parsed.common,
    foreman: parsed.foreman,
    peer: parsed.peer,
    voiceSecretary: trimBlock(note),
    actorNotes: parsed.actorNotes,
    actorOrder,
    extraTaggedBlocks: parsed.extraTaggedBlocks,
  });
}
