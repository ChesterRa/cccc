/** Whether a read failed without establishing that access was rejected. */
export function isTransientApiFailure(error: { code: string; httpStatus?: number }): boolean {
  const status = error.httpStatus;
  // A response body can be interrupted even when the HTTP status is already known.
  if (status !== undefined && status >= 400 && status < 500) return false;
  return error.code === "NETWORK_ERROR" || (status !== undefined && status >= 500);
}
