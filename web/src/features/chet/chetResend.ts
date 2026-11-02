import { v5 as uuidV5 } from "uuid";

/**
 * A not-started input has one successor. Every tab and retry derives the same ID, so
 * the host's existing registration check can admit that successor only once. If the
 * successor also did not start, its own ID can name a further attempt.
 */
export function resendId(inputId: string): string {
  return uuidV5(`cccc:chet-resend:v1:${inputId.toLowerCase()}`, uuidV5.URL);
}
