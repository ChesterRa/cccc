/** Whether `at` is the @ in a recognizable email address, not a mention. */
export function isEmailAt(text: string, at: number): boolean {
  if (text[at] !== "@" || at < 1 || !/[\p{L}\p{N}\p{M}._%+-]/u.test(text[at - 1])) return false;
  const pattern = /[\p{L}\p{N}\p{M}.-]+/uy;
  pattern.lastIndex = at + 1;
  const domain = pattern.exec(text)?.[0] || "";
  let end = domain.length;
  while (end > 0 && domain[end - 1] === ".") end -= 1;
  const dot = domain.lastIndexOf(".", end - 1);
  if (dot < 1) return false;
  const suffix = domain.slice(dot + 1, end);
  return /^(?:\p{L}\p{M}*){2,}$/u.test(suffix) || /^xn--[a-z0-9-]+$/iu.test(suffix);
}
