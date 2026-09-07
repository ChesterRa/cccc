/** Keep transport errors behind previously received transcript/closed messages. */
export function queueVoiceSocketError(
  messages: Promise<void>,
  shouldIgnore: () => boolean,
  reportError: () => void,
): Promise<void> {
  return messages.then(() => {
    if (shouldIgnore()) return;
    reportError();
  });
}
