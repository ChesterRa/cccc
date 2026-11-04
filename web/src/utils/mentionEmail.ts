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
  // CJK prose often joins an @name directly to the next word. Prefer the
  // mention when that first label mixes ASCII with CJK; ordinary Unicode
  // addresses whose labels stay in one script still count as email.
  const cjk =
    /[\p{Script=Han}\p{Script_Extensions=Hiragana}\p{Script_Extensions=Katakana}\p{Script=Hangul}]/u;
  const firstLabel = domain.slice(0, domain.indexOf("."));
  if (cjk.test(text[at - 1]) && cjk.test(firstLabel) && /[A-Za-z]/u.test(firstLabel)) {
    return false;
  }
  const suffix = domain.slice(dot + 1, end);
  return /^(?:\p{L}\p{M}*){2,}$/u.test(suffix) || /^xn--[a-z0-9-]+$/iu.test(suffix);
}
