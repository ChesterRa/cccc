import type { Summary } from "./usePendingSummary";

export type PendingIndicatorTone = "attention" | "chet" | "unknown";

export type PendingIndicator = {
  text: string;
  state: "fresh" | "stale" | "unknown";
  count: number | null;
  /** Visual emphasis: omitted/"attention" = needs user (or unattended off), "chet" = Chet only, "unknown" = breakdown uncertain with U=0. */
  tone?: PendingIndicatorTone;
  needsUserCount?: number;
  withChetCount?: number;
  unknownCount?: number;
  /** Unknown because the running daemon does not support the count. */
  daemonUnsupported?: true;
};

type Translator = (key: string, options?: Record<string, unknown>) => string;

function countText(count: number): string {
  return count > 99 ? "99+" : String(count);
}

export function pendingIndicatorTone(indicator: PendingIndicator): PendingIndicatorTone {
  return indicator.tone ?? "attention";
}

export function pendingIndicatorStatusLabel(t: Translator, indicator: PendingIndicator): string {
  if (indicator.daemonUnsupported) return t("pendingRepliesDaemonUnsupported");
  const tone = pendingIndicatorTone(indicator);
  if (tone === "chet") {
    return indicator.state === "stale"
      ? t("pendingRepliesWithChetStale", { value: indicator.text })
      : t("pendingRepliesWithChet", { value: indicator.text });
  }
  if (tone === "unknown") {
    return indicator.state === "stale"
      ? t("pendingRepliesStale")
      : t("pendingRepliesUnattendedUnknown");
  }
  if (indicator.state === "stale") return t("pendingRepliesStale");
  if ((indicator.needsUserCount ?? 0) > 0 && (indicator.unknownCount ?? 0) > 0) {
    return t("pendingRepliesNeedsUserWithUnknown", { count: indicator.needsUserCount });
  }
  if ((indicator.needsUserCount ?? 0) > 0 && (indicator.withChetCount ?? 0) > 0) {
    return t("pendingRepliesNeedsUserWithChet", {
      count: indicator.needsUserCount,
      withChet: indicator.withChetCount,
    });
  }
  if (indicator.state === "fresh") return t("pendingRepliesCount", { value: indicator.text });
  return t("pendingRepliesUnknown");
}

export function pendingIndicatorActionLabel(t: Translator, indicator: PendingIndicator): string {
  if (indicator.daemonUnsupported) return t("openPendingRepliesDaemonUnsupported");
  const tone = pendingIndicatorTone(indicator);
  if (tone === "chet") return t("openPendingRepliesWithChet", { value: indicator.text });
  if (tone === "unknown") {
    return indicator.state === "stale"
      ? t("openPendingRepliesStale")
      : t("openPendingRepliesUnattendedUnknown");
  }
  if (indicator.state === "stale") return t("openPendingRepliesStale");
  if (indicator.state === "fresh") return t("openPendingRepliesCount", { value: indicator.text });
  return t("openPendingRepliesUnknown");
}

export function pendingIndicatorSummaryLabel(t: Translator, indicator: PendingIndicator): string {
  const tone = pendingIndicatorTone(indicator);
  if (tone === "chet") {
    return indicator.state === "stale"
      ? t("pendingRepliesWithChetStale", { value: indicator.text })
      : t("pendingRepliesWithChet", { value: indicator.text });
  }
  if (tone === "unknown") {
    return indicator.state === "stale"
      ? t("pendingRepliesStale")
      : t("pendingRepliesUnattendedUnknown");
  }
  return t("pendingRepliesIndicator", { value: indicator.text });
}

export function pendingIndicator(summary: Summary | undefined): PendingIndicator | null {
  if (!summary || summary.state === "unknown" || summary.pending === null)
    return summary?.daemonUnsupported
      ? { text: "?", state: "unknown", count: null, daemonUnsupported: true }
      : { text: "?", state: "unknown", count: null };
  if (!summary.unattended) {
    if (summary.state === "stale")
      return {
        text: summary.pending > 0 ? `~${countText(summary.pending)}` : "?",
        state: "stale",
        count: summary.pending,
      };
    return summary.pending > 0
      ? { text: countText(summary.pending), state: "fresh", count: summary.pending }
      : null;
  }
  const { unattended } = summary;
  if (summary.state === "stale") {
    if (summary.pending === 0) return { text: "?", state: "stale", count: 0, tone: "unknown" };
    if (unattended.state === "unknown") {
      return {
        text: "?",
        state: "stale",
        count: null,
        tone: "unknown",
        unknownCount: summary.pending,
      };
    }
    const { needsUser: u, withChet: c, unknown: k } = unattended;
    if (u > 0 && k === 0) {
      return {
        text: `~${countText(u)}`,
        state: "stale",
        count: u,
        tone: "attention",
        needsUserCount: u,
        withChetCount: c,
        unknownCount: 0,
      };
    }
    if (u > 0 && k > 0) {
      return {
        text: `~${countText(u)}+?`,
        state: "stale",
        count: u,
        tone: "attention",
        needsUserCount: u,
        withChetCount: c,
        unknownCount: k,
      };
    }
    if (u === 0 && k > 0) {
      return {
        text: "?",
        state: "stale",
        count: null,
        tone: "unknown",
        needsUserCount: 0,
        withChetCount: c,
        unknownCount: k,
      };
    }
    if (u === 0 && k === 0 && c > 0) {
      return {
        text: `~${countText(c)}`,
        state: "stale",
        count: c,
        tone: "chet",
        needsUserCount: 0,
        withChetCount: c,
        unknownCount: 0,
      };
    }
    return {
      text: "?",
      state: "stale",
      count: null,
      tone: "unknown",
      unknownCount: summary.pending,
    };
  }
  if (summary.pending === 0) return null;
  if (unattended.state === "unknown") {
    return {
      text: "?",
      state: "unknown",
      count: null,
      tone: "unknown",
      unknownCount: summary.pending,
    };
  }
  const { needsUser: u, withChet: c, unknown: k } = unattended;
  if (u > 0 && k === 0) {
    return {
      text: countText(u),
      state: "fresh",
      count: u,
      tone: "attention",
      needsUserCount: u,
      withChetCount: c,
      unknownCount: 0,
    };
  }
  if (u > 0 && k > 0) {
    return {
      text: `${countText(u)}+?`,
      state: "fresh",
      count: u,
      tone: "attention",
      needsUserCount: u,
      withChetCount: c,
      unknownCount: k,
    };
  }
  if (u === 0 && k > 0) {
    return {
      text: "?",
      state: "unknown",
      count: null,
      tone: "unknown",
      needsUserCount: 0,
      withChetCount: c,
      unknownCount: k,
    };
  }
  if (u === 0 && k === 0 && c > 0) {
    return {
      text: countText(c),
      state: "fresh",
      count: c,
      tone: "chet",
      needsUserCount: 0,
      withChetCount: c,
      unknownCount: 0,
    };
  }
  return {
    text: "?",
    state: "unknown",
    count: null,
    tone: "unknown",
    unknownCount: summary.pending,
  };
}

/** Keep the active filter visible after the last answer until the user leaves it. */
export function pendingFilterIndicator(
  summary: Summary | undefined,
  active: boolean,
): PendingIndicator | null {
  return pendingIndicator(summary) ?? (active ? { text: "0", state: "fresh", count: 0 } : null);
}

export function aggregatePendingIndicator(
  summaries: Record<string, Summary>,
  groupIds: readonly string[],
): PendingIndicator | null {
  if (!groupIds.length) return null;
  let anyUnattended = false;
  let totalU = 0;
  let totalC = 0;
  let totalK = 0;
  let outerUnknown = false;
  let breakdownUnknown = false;
  let daemonUnsupported = false;
  for (const id of new Set(groupIds)) {
    const summary = summaries[id];
    if (summary?.daemonUnsupported) daemonUnsupported = true;
    if (summary?.unattended) anyUnattended = true;
    if (!summary || summary.state !== "fresh" || summary.pending === null) {
      outerUnknown = true;
      continue;
    }
    if (summary.pending === 0) continue;
    if (!summary.unattended) {
      totalU = Math.min(Number.MAX_SAFE_INTEGER, totalU + summary.pending);
    } else if (summary.unattended.state === "unknown") {
      breakdownUnknown = true;
      totalK = Math.min(Number.MAX_SAFE_INTEGER, totalK + summary.pending);
    } else {
      const { needsUser: u, withChet: c, unknown: k } = summary.unattended;
      totalU = Math.min(Number.MAX_SAFE_INTEGER, totalU + u);
      totalC = Math.min(Number.MAX_SAFE_INTEGER, totalC + c);
      if (k > 0) {
        breakdownUnknown = true;
        totalK = Math.min(Number.MAX_SAFE_INTEGER, totalK + k);
      }
      if (u === 0 && c === 0 && k === 0) {
        breakdownUnknown = true;
        totalK = Math.min(Number.MAX_SAFE_INTEGER, totalK + summary.pending);
      }
    }
  }
  if (!anyUnattended) {
    if (outerUnknown) {
      const text = totalU ? `${countText(totalU)}+?` : "?";
      return daemonUnsupported
        ? { text, state: "unknown", count: null, daemonUnsupported: true }
        : { text, state: "unknown", count: null };
    }
    return totalU ? { text: countText(totalU), state: "fresh", count: totalU } : null;
  }
  if (outerUnknown) {
    const text = totalU ? `${countText(totalU)}+?` : "?";
    return {
      text,
      state: "unknown",
      count: null,
      tone: "attention",
      ...(daemonUnsupported ? { daemonUnsupported: true as const } : {}),
    };
  }
  if (totalU > 0 && !breakdownUnknown) {
    return {
      text: countText(totalU),
      state: "fresh",
      count: totalU,
      tone: "attention",
      needsUserCount: totalU,
      withChetCount: totalC,
      unknownCount: 0,
    };
  }
  if (totalU > 0 && breakdownUnknown) {
    return {
      text: `${countText(totalU)}+?`,
      state: "unknown",
      count: null,
      tone: "attention",
      needsUserCount: totalU,
      withChetCount: totalC,
      unknownCount: totalK,
    };
  }
  if (breakdownUnknown) {
    return {
      text: "?",
      state: "unknown",
      count: null,
      tone: "unknown",
      needsUserCount: 0,
      withChetCount: totalC,
      unknownCount: totalK,
    };
  }
  if (totalC > 0) {
    return {
      text: countText(totalC),
      state: "fresh",
      count: totalC,
      tone: "chet",
      needsUserCount: 0,
      withChetCount: totalC,
      unknownCount: 0,
    };
  }
  return null;
}
