export type RuntimeIndicatorTone = "stop" | "run" | "working";

export type RuntimeIndicatorState = {
  tone: RuntimeIndicatorTone;
  dotClass: string;
  labelClass: string;
  pulse: boolean;
  strongPulse: boolean;
};

export type GroupPresenceTone = "run" | "paused" | "idle" | "stop";

export const STOPPED_INDICATOR_DOT_CLASS = "bg-slate-400/65 dark:bg-slate-500/70";

export const QUIET_RUN_INDICATOR_DOT_CLASS = "bg-emerald-500/70 dark:bg-emerald-400/75";

const WORKING_INDICATOR_DOT_CLASS =
  "bg-emerald-500 ring-[1.5px] ring-emerald-200/90 shadow-[0_0_0_3px_rgba(16,185,129,0.12),0_0_18px_rgba(52,211,153,0.75)] dark:bg-emerald-400 dark:ring-emerald-400/35 dark:shadow-[0_0_0_3px_rgba(16,185,129,0.10),0_0_18px_rgba(52,211,153,0.75)] scale-110";

const PAUSED_INDICATOR_DOT_CLASS = "bg-amber-500/80 dark:bg-amber-400/80";

const IDLE_INDICATOR_DOT_CLASS = "bg-sky-500/70 dark:bg-sky-400/75";

export function getRuntimeIndicatorState(input: {
  isRunning: boolean;
  workingState: string;
}): RuntimeIndicatorState {
  const workingState = String(input.workingState || "")
    .trim()
    .toLowerCase();
  if (!input.isRunning) {
    return {
      tone: "stop",
      dotClass: STOPPED_INDICATOR_DOT_CLASS,
      labelClass: "",
      pulse: false,
      strongPulse: false,
    };
  }

  if (workingState === "working") {
    return {
      tone: "working",
      dotClass: WORKING_INDICATOR_DOT_CLASS,
      labelClass: "text-emerald-700 dark:text-emerald-300",
      pulse: true,
      strongPulse: true,
    };
  }

  return {
    tone: "run",
    dotClass: QUIET_RUN_INDICATOR_DOT_CLASS,
    labelClass: "",
    pulse: false,
    strongPulse: false,
  };
}

export function getGroupPresenceDotClass(tone: GroupPresenceTone): string {
  switch (tone) {
    case "paused":
      return PAUSED_INDICATOR_DOT_CLASS;
    case "idle":
      return IDLE_INDICATOR_DOT_CLASS;
    case "stop":
      return STOPPED_INDICATOR_DOT_CLASS;
    case "run":
    default:
      return QUIET_RUN_INDICATOR_DOT_CLASS;
  }
}
