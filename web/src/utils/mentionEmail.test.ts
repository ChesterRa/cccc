import { expect, it } from "vite-plus/test";
import { isEmailAt } from "./mentionEmail";

it.each([
  "cafe\u0301@example.com",
  "名字@exem\u0301ple.com",
  "名字@例子.中国",
  "名字@example.com",
  "alice+tag@example.com.",
  "(名字@例子.中国)。",
  "alice@example.xn--fiqs8s",
  "名字@sub.example.co.jp",
  "名字@例子123.中国",
  "名字@例子-123.中国",
  "alice@abc中文.com",
])("recognizes a dotted email address inside %s", (text) => {
  expect(isEmailAt(text, text.indexOf("@"))).toBe(true);
});

it.each([
  "@worker",
  "请@worker",
  "请@worker-1",
  "请@codex-1看一下README.md",
  "请@codex-1看一下。README.md",
  "user@localhost",
  "#group",
  "@.com",
])("does not mistake %s for a dotted address", (text) => {
  expect(isEmailAt(text, text.indexOf("@"))).toBe(false);
});
