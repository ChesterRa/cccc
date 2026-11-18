/** Keys in `actors` for a terminal that needs the user before CCCC connects: Claude asks for a
 *  confirmation there and Hermes runs its first setup, with the same states. Any other terminal
 *  pauses only at its startup input. */
export function runtimeConfirmationKeys(runtime?: string) {
  const setup = runtime === "hermes";
  const claude = runtime === "claude";
  return {
    pending: setup ? "runtimeSetupPending" : "runtimeConfirmationPending",
    hint: setup ? "runtimeSetupHint" : "runtimeConfirmationHint",
    required: setup ? "runtimeSetupRequired" : "runtimeConfirmationRequired",
    openTerminal: setup ? "runtimeSetupOpenTerminal" : "runtimeConfirmationOpenTerminal",
    paused: setup
      ? "runtimeSetupPaused"
      : claude
        ? "runtimeConfirmationPaused"
        : "runtimeStartupInputPaused",
    pausedHint: setup
      ? "runtimeSetupPausedHint"
      : claude
        ? "runtimeConfirmationPausedHint"
        : "runtimeStartupInputPausedHint",
  };
}
