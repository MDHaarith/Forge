/// <reference types="vite/client" />
// Markdown with LaTeX math ($…$, $$…$$, \(…\), \[…\]) for chat replies and .md files.
import DOMPurify from "dompurify";
import katex from "katex";
import "katex/dist/katex.min.css";
import { marked, type TokenizerAndRendererExtension } from "marked";

const tex = (text: string, displayMode: boolean) => katex.renderToString(text, { displayMode, throwOnError: false });

const math = (name: string, level: "block" | "inline", rule: RegExp, start: RegExp): TokenizerAndRendererExtension => ({
  name,
  level,
  start: (src) => src.match(start)?.index,
  tokenizer(src) {
    const m = rule.exec(src);
    if (m) return { type: name, raw: m[0], text: (m[1] ?? m[2]).trim() };
  },
  renderer: (token) => tex(token.text, level === "block") + (level === "block" ? "\n" : ""),
});

marked.use({
  gfm: true,
  breaks: true,
  extensions: [
    math("mathBlock", "block", /^(?:\$\$([\s\S]+?)\$\$|\\\[([\s\S]+?)\\\])[ \t]*(?:\n|$)/, /\$\$|\\\[/),
    // Inline $…$ needs non-space just inside both dollars and no digit after, so "$5 and $6" stays text.
    math("mathInline", "inline", /^(?:\$(?!\s)((?:\\\$|[^$\n])+?)(?<!\s)\$(?!\d)|\\\(([\s\S]+?)\\\))/, /\$|\\\(/),
  ],
});

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noopener noreferrer");
  }
});

/** Markdown source to sanitized HTML. */
export function renderMarkdown(source: string): string {
  return DOMPurify.sanitize(marked.parse(source, { async: false }));
}
