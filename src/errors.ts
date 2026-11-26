/** `val` as an `Error`: itself if it is one, otherwise its text in a new one. */
export function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}
