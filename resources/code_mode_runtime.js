const readline = require("node:readline");
const vm = require("node:vm");

const EXIT_SENTINEL = "__cccc_code_mode_exit__";
let pending = new Map();
let storedValues = {};
let nextToolId = 1;
let started = false;

function send(payload) {
  process.stdout.write(JSON.stringify(payload) + "\n");
}

function finish(errorText = "", storedValuesOverride = null) {
  const values = storedValuesOverride && typeof storedValuesOverride === "object" ? storedValuesOverride : storedValues;
  const payload = JSON.stringify({ type: "result", stored_values: values, error_text: errorText }) + "\n";
  process.stdout.write(payload, () => process.exit(0));
}

function jsonString(value) {
  try {
    const text = JSON.stringify(value === undefined ? null : value);
    return typeof text === "string" ? text : "null";
  } catch (_err) {
    return "null";
  }
}

function parseJsonObject(text, fallback = {}) {
  try {
    const parsed = JSON.parse(String(text || "{}"));
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : fallback;
  } catch (_err) {
    return fallback;
  }
}

function hardenFunction(fn) {
  Object.setPrototypeOf(fn, null);
  return Object.freeze(fn);
}

function buildBridge() {
  const bridge = Object.create(null);
  Object.defineProperties(bridge, {
    content: {
      value: hardenFunction((itemJson) => {
        send({ type: "content", item: JSON.parse(String(itemJson || "{}")) });
      }),
    },
    toolCall: {
      value: hardenFunction((rawName, payloadJson, resolveJson, rejectMessage) => {
        const id = `tool-${nextToolId++}`;
        let payload = null;
        try {
          payload = JSON.parse(String(payloadJson || "null"));
        } catch (err) {
          rejectMessage(String(err && err.message || err));
          return;
        }
        pending.set(id, { resolveJson, rejectMessage });
        send({ type: "tool_call", id, name: String(rawName || ""), input: payload });
      }),
    },
    yield: {
      value: hardenFunction((storedJson) => {
        send({ type: "yield", stored_values: parseJsonObject(storedJson, {}) });
      }),
    },
    setTimeout: { value: hardenFunction((callback, ms, ...args) => setTimeout(callback, ms, ...args)) },
    clearTimeout: { value: hardenFunction((id) => clearTimeout(id)) },
  });
  return Object.freeze(bridge);
}

function buildContext(toolsMetadata, initialStoredValues, workLoops, helpAliases, helpCompactNotes, helpCuratedTools, helpCuratedLoops) {
  const sandbox = Object.create(null);
  Object.defineProperties(sandbox, {
    __cccc_bridge__: { value: buildBridge(), configurable: true },
    __cccc_tools_metadata_json__: { value: JSON.stringify(Array.isArray(toolsMetadata) ? toolsMetadata : []), configurable: true },
    __cccc_work_loops_json__: { value: JSON.stringify(Array.isArray(workLoops) ? workLoops : []), configurable: true },
    __cccc_help_aliases_json__: { value: JSON.stringify(helpAliases && typeof helpAliases === "object" ? helpAliases : {}), configurable: true },
    __cccc_help_compact_notes_json__: { value: JSON.stringify(helpCompactNotes && typeof helpCompactNotes === "object" ? helpCompactNotes : {}), configurable: true },
    __cccc_help_curated_tools_json__: { value: JSON.stringify(helpCuratedTools && typeof helpCuratedTools === "object" ? helpCuratedTools : {}), configurable: true },
    __cccc_help_curated_loops_json__: { value: JSON.stringify(helpCuratedLoops && typeof helpCuratedLoops === "object" ? helpCuratedLoops : {}), configurable: true },
    __cccc_stored_values_json__: { value: jsonString(initialStoredValues && typeof initialStoredValues === "object" ? initialStoredValues : {}), configurable: true },
    __cccc_exit_sentinel__: { value: EXIT_SENTINEL, configurable: true },
    constructor: { value: undefined, configurable: true },
    console: { value: undefined, configurable: true },
    require: { value: undefined, configurable: true },
    process: { value: undefined, configurable: true },
    fetch: { value: undefined, configurable: true },
    WebSocket: { value: undefined, configurable: true },
  });
  const context = vm.createContext(sandbox, {
    name: "cccc_code_mode",
    codeGeneration: { strings: false, wasm: false },
  });
  const bootstrap = `
(() => {
  const bridge = globalThis.__cccc_bridge__;
  const toolsMetadata = JSON.parse(globalThis.__cccc_tools_metadata_json__ || "[]");
  const commonWorkLoops = Object.freeze(JSON.parse(globalThis.__cccc_work_loops_json__ || "[]"));
  const helpAliases = JSON.parse(globalThis.__cccc_help_aliases_json__ || "{}");
  const helpCompactNotes = JSON.parse(globalThis.__cccc_help_compact_notes_json__ || "{}");
  const helpCuratedTools = JSON.parse(globalThis.__cccc_help_curated_tools_json__ || "{}");
  const helpCuratedLoops = JSON.parse(globalThis.__cccc_help_curated_loops_json__ || "{}");
  const exitSentinel = String(globalThis.__cccc_exit_sentinel__ || "");
  let storedValues = JSON.parse(globalThis.__cccc_stored_values_json__ || "{}");
  delete globalThis.__cccc_bridge__;
  delete globalThis.__cccc_tools_metadata_json__;
  delete globalThis.__cccc_work_loops_json__;
  delete globalThis.__cccc_help_aliases_json__;
  delete globalThis.__cccc_help_compact_notes_json__;
  delete globalThis.__cccc_help_curated_tools_json__;
  delete globalThis.__cccc_help_curated_loops_json__;
  delete globalThis.__cccc_stored_values_json__;
  delete globalThis.__cccc_exit_sentinel__;

  function define(name, value) {
    Object.defineProperty(globalThis, name, {
      value,
      writable: false,
      configurable: false,
      enumerable: false,
    });
  }

  define("constructor", undefined);
  define("console", undefined);
  define("require", undefined);
  define("process", undefined);
  define("fetch", undefined);
  define("WebSocket", undefined);

  function stringify(value) {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value);
    } catch (_err) {
      return String(value);
    }
  }

  function cloneSerializable(value, label) {
    if (value === undefined) return undefined;
    try {
      return JSON.parse(JSON.stringify(value));
    } catch (_err) {
      throw new TypeError(label + " must be JSON-serializable");
    }
  }

  const tools = Object.create(null);
  for (const tool of toolsMetadata) {
    const globalName = String(tool.global_name || "");
    const rawName = String(tool.name || "");
    if (!globalName || !rawName) continue;
    Object.defineProperty(tools, globalName, {
      enumerable: true,
      value(input = {}) {
        let payloadJson = "null";
        try {
          const payload = input === undefined ? null : cloneSerializable(input, rawName + " input");
          payloadJson = JSON.stringify(payload);
        } catch (err) {
          return Promise.reject(err);
        }
        return new Promise((resolve, reject) => {
          bridge.toolCall(
            rawName,
            payloadJson,
            (resultJson) => {
              try {
                resolve(JSON.parse(String(resultJson || "null")));
              } catch (err) {
                reject(err);
              }
            },
            (message) => reject(new Error(String(message || "tool call failed")))
          );
        });
      },
    });
  }
  Object.freeze(tools);

  const allTools = toolsMetadata.map((tool) => Object.freeze({
    name: String(tool.global_name || ""),
    raw_name: String(tool.name || ""),
    description: String(tool.description || ""),
  }));
  define("tools", tools);
  define("ALL_TOOLS", Object.freeze(allTools));
  define("COMMON_WORK_LOOPS", commonWorkLoops);
  function normalizeHelpOptions(options) {
    if (options && typeof options === "object" && !Array.isArray(options)) return options;
    return {};
  }
  function queryTokens(query) {
    const needle = String(query || "").trim().toLowerCase();
    if (!needle) return [];
    const tokens = new Set([needle]);
    for (const part of needle.split(/[^a-z0-9_]+/).filter(Boolean)) tokens.add(part);
    const aliases = helpAliases && Array.isArray(helpAliases[needle]) ? helpAliases[needle] : [];
    for (const alias of aliases) tokens.add(String(alias || "").trim().toLowerCase());
    for (const [key, values] of Object.entries(helpAliases || {})) {
      if (Array.isArray(values) && values.map((item) => String(item || "").trim().toLowerCase()).includes(needle)) {
        tokens.add(String(key || "").trim().toLowerCase());
      }
    }
    return Array.from(tokens).filter(Boolean);
  }
  function canonicalQuery(tokens) {
    for (const token of tokens) {
      if (Array.isArray(helpCuratedTools[token]) || Array.isArray(helpCuratedLoops[token])) return token;
    }
    return tokens.length ? tokens[0] : "";
  }
  function rankByCurated(items, names, getName) {
    if (!Array.isArray(names) || !names.length) return items;
    const rank = new Map(names.map((name, index) => [String(name || ""), index]));
    const selected = [];
    const rest = [];
    for (const item of items) {
      if (rank.has(String(getName(item) || ""))) selected.push(item);
      else rest.push(item);
    }
    selected.sort((a, b) => (rank.get(String(getName(a) || "")) || 0) - (rank.get(String(getName(b) || "")) || 0));
    return selected.concat(rest);
  }
  function toolMatches(tool, tokens) {
    if (!tokens.length) return true;
    const haystack = [tool.name, tool.raw_name, tool.description].join(" ").toLowerCase();
    return tokens.some((token) => haystack.includes(token));
  }
  function loopMatches(loop, tokens) {
    if (!tokens.length) return true;
    const haystack = [loop && loop.name || "", Array.isArray(loop && loop.steps) ? loop.steps.join(" ") : ""].join(" ").toLowerCase();
    return tokens.some((token) => haystack.includes(token));
  }
  function compactTool(tool) {
    return {
      name: tool.name,
      raw_name: tool.raw_name,
      summary: String(tool.description || "").split(/\\n/)[0].slice(0, 320),
    };
  }
  function compactNotes(tokens) {
    const notes = [];
    const seen = new Set();
    for (const token of tokens.length ? tokens : ["code_exec"]) {
      const values = Array.isArray(helpCompactNotes[token]) ? helpCompactNotes[token] : [];
      for (const value of values) {
        const text = String(value || "").trim();
        if (text && !seen.has(text)) {
          notes.push(text);
          seen.add(text);
        }
      }
    }
    return notes.slice(0, 12);
  }
  function matchingTools(query) {
    const tokens = queryTokens(query);
    const canonical = canonicalQuery(tokens);
    const curated = Array.isArray(helpCuratedTools[canonical]) ? helpCuratedTools[canonical] : [];
    const matched = allTools.filter((tool) => toolMatches(tool, tokens));
    if (curated.length) {
      const curatedSet = new Set(curated.map((name) => String(name || "")));
      const exact = allTools.filter((tool) => curatedSet.has(tool.raw_name));
      const selected = exact.length ? exact : matched;
      const seen = new Set();
      return rankByCurated(selected, curated, (tool) => tool.raw_name).filter((tool) => {
        const key = tool.raw_name;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }
    return matched;
  }
  function matchingLoops(query) {
    const tokens = queryTokens(query);
    const canonical = canonicalQuery(tokens);
    const curated = Array.isArray(helpCuratedLoops[canonical]) ? helpCuratedLoops[canonical] : [];
    const matched = commonWorkLoops.filter((loop) => loopMatches(loop, tokens));
    if (curated.length) {
      const curatedSet = new Set(curated.map((name) => String(name || "")));
      const exact = commonWorkLoops.filter((loop) => curatedSet.has(String(loop && loop.name || "")));
      const selected = exact.length ? exact : matched;
      const seen = new Set();
      return rankByCurated(selected, curated, (loop) => String(loop && loop.name || "")).filter((loop) => {
        const key = String(loop && loop.name || "");
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }
    return matched;
  }
  define("tool_names", function tool_names(query = "") {
    return matchingTools(query).map((tool) => tool.name);
  });
  define("list_tools", function list_tools(query = "") {
    return matchingTools(query).map(compactTool).slice(0, 24);
  });
  define("tool_help", function tool_help(query = "", options = {}) {
    const opts = normalizeHelpOptions(options);
    const detail = String(opts.detail || opts.mode || "compact").trim().toLowerCase();
    const tokens = queryTokens(query);
    const matches = matchingTools(query).slice(0, 12);
    const loops = matchingLoops(query).slice(0, 6);
    return {
      tools: detail === "schema" || detail === "full" ? matches : matches.map(compactTool),
      common_work_loops: loops,
      notes: compactNotes(tokens),
      usage: "Call nested tools as await tools.<name>({...}). Prefer cccc_code_exec for multi-step read/patch/test/diff/report loops; use direct tools for one-step work.",
    };
  });
  define("text", function text(value) {
    bridge.content(JSON.stringify({ type: "text", text: stringify(value) }));
  });
  define("store", function store(key, value) {
    if (typeof key !== "string" || key.length === 0) {
      throw new TypeError("store key must be a non-empty string");
    }
    storedValues[key] = cloneSerializable(value, "stored value " + key);
  });
  define("load", function load(key) {
    return cloneSerializable(storedValues[String(key)], "stored value " + String(key));
  });
  define("yield_control", function yield_control() {
    bridge.yield(JSON.stringify(storedValues));
  });
  define("exit", function exit() {
    throw new Error(exitSentinel);
  });
  define("setTimeout", function ccccSetTimeout(callback, ms, ...args) {
    if (typeof callback !== "function") {
      throw new TypeError("setTimeout callback must be a function");
    }
    return bridge.setTimeout(callback, Number(ms) || 0, ...args);
  });
  define("clearTimeout", function ccccClearTimeout(id) {
    return bridge.clearTimeout(id);
  });
  define("__cccc_export_stored_values__", function __cccc_export_stored_values__() {
    return JSON.stringify(storedValues);
  });
})();
`;
  new vm.Script(bootstrap, { filename: "cccc_code_exec_bootstrap.mjs" }).runInContext(context, { timeout: 2000 });
  return context;
}

function exportStoredValues(context) {
  if (!context) return storedValues;
  try {
    const raw = vm.runInContext("__cccc_export_stored_values__()", context, { timeout: 1000 });
    return parseJsonObject(raw, {});
  } catch (_err) {
    return {};
  }
}

async function startCell(command) {
  if (started) {
    finish("cell already started");
    return;
  }
  started = true;
  storedValues = command.stored_values && typeof command.stored_values === "object" ? command.stored_values : {};
  let context = null;
  const source = String(command.source || "");
  send({ type: "started" });
  try {
    context = buildContext(
      Array.isArray(command.tools) ? command.tools : [],
      storedValues,
      Array.isArray(command.work_loops) ? command.work_loops : [],
      command.help_aliases && typeof command.help_aliases === "object" ? command.help_aliases : {},
      command.help_compact_notes && typeof command.help_compact_notes === "object" ? command.help_compact_notes : {},
      command.help_curated_tools && typeof command.help_curated_tools === "object" ? command.help_curated_tools : {},
      command.help_curated_loops && typeof command.help_curated_loops === "object" ? command.help_curated_loops : {}
    );
    const script = new vm.Script(`(async () => {\n${source}\n})()`, {
      filename: "cccc_code_exec.mjs",
    });
    await script.runInContext(context, { timeout: 2000 });
    finish("", exportStoredValues(context));
  } catch (err) {
    const message = err && err.message === EXIT_SENTINEL ? "" : (err && (err.stack || err.message)) || String(err);
    finish(message, exportStoredValues(context));
  }
}

function resolveToolResponse(command) {
  const id = String(command.id || "");
  const entry = pending.get(id);
  if (!entry) return;
  pending.delete(id);
  if (command.ok) {
    entry.resolveJson(jsonString(command.result));
  } else {
    entry.rejectMessage(String(command.error || "tool call failed"));
  }
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  let command = null;
  try {
    command = JSON.parse(line);
  } catch (err) {
    finish(`invalid runtime command: ${err.message}`);
    return;
  }
  if (!command || typeof command !== "object") return;
  if (command.type === "start") {
    startCell(command);
  } else if (command.type === "tool_response") {
    resolveToolResponse(command);
  } else if (command.type === "terminate") {
    process.exit(0);
  }
});
