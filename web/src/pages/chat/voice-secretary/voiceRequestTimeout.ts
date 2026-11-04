/** A timeout ends the UI wait, not the server operation. Its outcome remains unknown. */
export async function waitForVoiceRequest<T>(
  request: Promise<T>,
  timeoutMs: number,
): Promise<{ timedOut: false; value: T } | { timedOut: true }> {
  let timer: number | undefined;
  try {
    return await Promise.race([
      request.then((value) => ({ timedOut: false as const, value })),
      new Promise<{ timedOut: true }>((resolve) => {
        timer = window.setTimeout(() => resolve({ timedOut: true }), timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== undefined) window.clearTimeout(timer);
  }
}
