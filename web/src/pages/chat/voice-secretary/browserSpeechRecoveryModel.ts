const PASSIVE_RECOVERABLE_BROWSER_SPEECH_ERRORS = new Set(["network"]);

export function shouldScheduleBrowserSpeechErrorRestart(errorCode: string): boolean {
  return !PASSIVE_RECOVERABLE_BROWSER_SPEECH_ERRORS.has(String(errorCode || "").trim());
}

const QUIET_ERRORS = new Set(["no-speech", "aborted"]);

export function isQuietBrowserSpeechError(code: string): boolean {
  return QUIET_ERRORS.has(code.trim());
}

/** One budget per recording run; an error and its ensuing end count only once. */
export class BrowserSpeechRecoveryBudget {
  failures = 0;
  private countedCycle = false;
  private receivedResult = false;

  constructor(private readonly limit = 8) {}

  get exhausted(): boolean {
    return this.failures >= this.limit;
  }

  reset(): void {
    this.failures = 0;
    this.beginCycle();
  }

  beginCycle(): void {
    this.countedCycle = false;
    this.receivedResult = false;
  }

  recordResult(): void {
    this.failures = 0;
    this.countedCycle = false;
    this.receivedResult = true;
  }

  recordError(code: string): void {
    if (!isQuietBrowserSpeechError(code)) this.countFailure();
  }

  endCycle(durationMs: number): boolean {
    // A normal quiet session may end after a browser timeout. Repeated immediate
    // empty ends instead indicate a broken service/device, even without onerror.
    if (!this.receivedResult && durationMs >= 0 && durationMs < 1_000) this.countFailure();
    return this.exhausted;
  }

  private countFailure(): void {
    if (this.countedCycle) return;
    this.countedCycle = true;
    this.failures += 1;
  }
}
