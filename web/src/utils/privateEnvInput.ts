const ENV_KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

function unwrapOuterQuotes(value: string): string {
  const v = String(value || "").trim();
  if (
    v.length >= 2 &&
    ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'")))
  ) {
    return v.slice(1, -1).trim();
  }
  return v;
}

function splitStatements(line: string): string[] {
  // Split by semicolons outside quotes, so shell snippets like
  // `export A="x"; export B='y'` can be pasted directly.
  const out: string[] = [];
  let cur = "";
  let inSingle = false;
  let inDouble = false;
  let backslashes = 0;

  for (let i = 0; i < line.length; i++) {
    const ch = line[i];

    // Inline comment: treat `# ...` as comment only when outside quotes and
    // at token boundary.
    if (ch === "#" && !inSingle && !inDouble) {
      const prev = i > 0 ? line[i - 1] : "";
      if ((i === 0 || /\s/.test(prev)) && backslashes % 2 === 0) break;
    }

    if (ch === "\\") {
      backslashes += 1;
      cur += ch;
      continue;
    }

    if (ch === "'" && !inDouble) {
      if (inSingle) {
        inSingle = false;
        cur += ch;
        backslashes = 0;
        continue;
      }
      if (backslashes % 2 === 0) {
        inSingle = true;
        cur += ch;
        backslashes = 0;
        continue;
      }
      cur += ch;
      backslashes = 0;
      continue;
    }

    if (ch === '"' && !inSingle) {
      if (inDouble) {
        // Keep escaped quote inside double-quoted strings.
        if (backslashes % 2 === 1) {
          cur += ch;
          backslashes = 0;
          continue;
        }
        inDouble = false;
        cur += ch;
        backslashes = 0;
        continue;
      }
      if (backslashes % 2 === 0) {
        inDouble = true;
        cur += ch;
        backslashes = 0;
        continue;
      }
      cur += ch;
      backslashes = 0;
      continue;
    }

    if (ch === ";" && !inSingle && !inDouble && backslashes % 2 === 0) {
      const seg = cur.trim();
      if (seg) out.push(seg);
      cur = "";
      backslashes = 0;
      continue;
    }

    cur += ch;
    backslashes = 0;
  }

  const last = cur.trim();
  if (last) out.push(last);
  return out;
}

function unquoteValue(valueRaw: string): string {
  let v = String(valueRaw ?? "").trim();

  // Common paste: KEY="value";
  if (v.endsWith(";")) v = v.slice(0, -1).trim();

  if (
    v.length >= 2 &&
    ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'")))
  ) {
    const quote = v[0];
    v = v.slice(1, -1);
    if (quote === '"') {
      // Pragmatic unescape for typical shell-pasted values.
      v = v.replace(/\\([nrt"\\])/g, (_m, c: string) => {
        if (c === "n") return "\n";
        if (c === "r") return "\r";
        if (c === "t") return "\t";
        if (c === '"') return '"';
        if (c === "\\") return "\\";
        return c;
      });
    }
  }

  return v;
}

function normalizeSetStatement(statement: string): { key: string; value: string } | null {
  let line = String(statement || "").trim();
  if (!line || line.startsWith("#")) return null;
  if (line.startsWith("export ")) line = line.slice("export ".length).trim();

  const ps = line.match(/^\$env:([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/i);
  if (ps) {
    return { key: String(ps[1] || "").trim(), value: unquoteValue(String(ps[2] ?? "")) };
  }

  if (/^set\s+/i.test(line)) {
    line = line.replace(/^set\s+/i, "").trim();
    line = unwrapOuterQuotes(line);
  }

  const m = line.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);
  if (!m) return null;
  return { key: String(m[1] || "").trim(), value: unquoteValue(String(m[2] ?? "")) };
}

function normalizeUnsetStatement(statement: string): string | null {
  let line = String(statement || "").trim();
  if (!line || line.startsWith("#")) return null;
  if (line.startsWith("unset ")) line = line.slice("unset ".length).trim();
  if (line.startsWith("export ")) line = line.slice("export ".length).trim();

  const psRemove = line.match(/^remove-item\s+env:([A-Za-z_][A-Za-z0-9_]*)$/i);
  if (psRemove) return String(psRemove[1] || "").trim();

  const psNull = line.match(/^\$env:([A-Za-z_][A-Za-z0-9_]*)\s*=\s*\$null$/i);
  if (psNull) return String(psNull[1] || "").trim();

  if (/^set\s+/i.test(line)) {
    line = line.replace(/^set\s+/i, "").trim();
    line = unwrapOuterQuotes(line);
  }

  // Allow `KEY=` / `set KEY=` to mean unset when users paste shell snippets.
  if (line.includes("=")) line = line.split("=")[0].trim();
  if (line.endsWith(";")) line = line.slice(0, -1).trim();
  return line;
}

export function parsePrivateEnvSetText(
  text: string,
): { ok: true; setVars: Record<string, string> } | { ok: false; error: string } {
  const out: Record<string, string> = {};
  const lines = String(text || "").split("\n");

  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const statements = splitStatements(trimmed);
    for (const st of statements) {
      const parsed = normalizeSetStatement(st);
      if (!parsed) {
        return {
          ok: false,
          error: `Set line ${i + 1}: expected KEY=VALUE (supports export / set / $env: / quotes / semicolon)`,
        };
      }

      const key = parsed.key;
      if (!ENV_KEY_RE.test(key)) {
        return { ok: false, error: `Set line ${i + 1}: invalid env key` };
      }

      out[key] = parsed.value;
    }
  }

  return { ok: true, setVars: out };
}

export function parsePrivateEnvUnsetText(
  text: string,
): { ok: true; unsetKeys: string[] } | { ok: false; error: string } {
  const out: string[] = [];
  const seen = new Set<string>();
  const lines = String(text || "").split("\n");

  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const statements = splitStatements(trimmed);
    for (const st of statements) {
      const line = normalizeUnsetStatement(st);
      if (!line) continue;

      if (!ENV_KEY_RE.test(line)) {
        return { ok: false, error: `Unset line ${i + 1}: invalid env key` };
      }
      if (seen.has(line)) continue;
      seen.add(line);
      out.push(line);
    }
  }

  return { ok: true, unsetKeys: out };
}
