// Optional Linux measurements for the isolated Connect browser scenario.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
const ticksPerSecond = Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }));
const pageBytes = Number(execFileSync("getconf", ["PAGESIZE"], { encoding: "utf8" }));

function processTree(root) {
  const parents = execFileSync("ps", ["-eo", "pid=,ppid="], { encoding: "utf8" })
    .trim()
    .split("\n")
    .map((line) => line.trim().split(/\s+/).map(Number));
  const included = new Set([root]);
  for (let changed = true; changed; ) {
    changed = false;
    for (const [pid, parent] of parents)
      if (included.has(parent) && !included.has(pid)) {
        included.add(pid);
        changed = true;
      }
  }
  let cpuTicks = 0,
    rssBytes = 0,
    alive = 0;
  for (const pid of included) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      cpuTicks += Number(fields[11]) + Number(fields[12]);
      rssBytes += Number(fields[21]) * pageBytes;
      alive++;
    } catch {
      /* A fixture child may have exited between the two reads. */
    }
  }
  return { cpuTicks, rssBytes, alive };
}

export function connectMeasurements({ roots, traffic, requests, accountOrigin, file }) {
  const evidence = {
    platform: process.platform,
    groupsPerInstance: 10,
    visibleTerminals: 4,
    samples: [],
    actions: [],
  };
  const snapshot = async () => ({
    at: performance.now(),
    processes: Object.fromEntries(
      Object.entries(roots()).map(([name, pid]) => [name, processTree(pid)]),
    ),
    traffic: structuredClone(traffic),
    requestIndex: requests.length,
    account: await fetch(accountOrigin + "/__fixture/status").then((r) => r.json()),
  });
  const save = () => writeFileSync(file, JSON.stringify(evidence, null, 2));
  return {
    action(name, milliseconds) {
      evidence.actions.push({ name, milliseconds });
      save();
    },
    async sample(name, wait, milliseconds = 65000) {
      // Cover the daemon's 60-second directory refresh. Shorter windows can
      // falsely suggest that an idle or hidden entry has no account cost.
      await wait(3000);
      const before = await snapshot();
      await wait(milliseconds);
      const after = await snapshot();
      const seconds = (after.at - before.at) / 1000;
      const processes = Object.fromEntries(
        Object.entries(after.processes).map(([key, value]) => [
          key,
          {
            cpuPercentOfOneCore:
              (Math.max(0, value.cpuTicks - (before.processes[key]?.cpuTicks || 0)) /
                ticksPerSecond /
                seconds) *
              100,
            rssSumMiB: value.rssBytes / 1048576,
            processes: value.alive,
          },
        ]),
      );
      const account = {
        requests: after.account.requests - before.account.requests,
        requestBytes: after.account.requestBytes - before.account.requestBytes,
        responseBytes: after.account.responseBytes - before.account.responseBytes,
        providerCalls: after.account.providerCalls - before.account.providerCalls,
        sqlReads: after.account.sql.reads - before.account.sql.reads,
        sqlWrites: after.account.sql.writes - before.account.sql.writes,
        changedRows: after.account.sql.changedRows - before.account.sql.changedRows,
      };
      const counts = {};
      for (const request of requests.slice(before.requestIndex)) {
        const key = `${request.index}:${request.method}:${request.path}`;
        counts[key] = (counts[key] || 0) + 1;
      }
      const network = after.traffic.map((row, index) => ({
        requestBytes: row.requestBytes - before.traffic[index].requestBytes,
        responseBytes: row.responseBytes - before.traffic[index].responseBytes,
        httpOpen: row.httpOpen,
        wsOpen: row.wsOpen,
      }));
      const result = { name, seconds, processes, network, requests: counts, account };
      evidence.samples.push(result);
      save();
      process.stdout.write(
        `Measurement ${name}: ${JSON.stringify({ seconds, processes, network, account })}\n`,
      );
      return result;
    },
  };
}
