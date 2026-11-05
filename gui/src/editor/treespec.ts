// CodeMirror 6 setup for TreeSpec drafts (design §6.5): highlighting, two
// spaces per Tab, an error-line decoration, and a read-only review marker.

import { defaultKeymap, history, historyKeymap, indentLess } from "@codemirror/commands";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { Compartment, EditorState, type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, EditorView, keymap, lineNumbers } from "@codemirror/view";
import { tags } from "@lezer/highlight";

export const MARKER = "# organizer-review-v1: ";

const language = StreamLanguage.define<{ desc: boolean }>({
  startState: () => ({ desc: false }),
  token(stream, state) {
    if (stream.sol()) {
      state.desc = false;
      if (stream.match("!tree")) return (stream.skipToEnd(), "meta");
      if (stream.match(/^\s*#/)) return (stream.skipToEnd(), "comment");
      if (stream.eatSpace()) return null;
    }
    if (state.desc) return (stream.skipToEnd(), "comment");
    if (stream.match(/^"(?:[^"\\]|\\.)*"/)) return "string";
    if (stream.match(/^ @[A-Za-z0-9_-]+/)) return "atom";
    if (stream.match(/^ #/)) return ((state.desc = true), "comment");
    if (stream.match(/^\/(?= |$)/)) return "keyword";
    stream.next();
    return "variableName";
  },
});

const highlight = HighlightStyle.define([
  { tag: tags.meta, color: "var(--rev)", fontWeight: "600" },
  { tag: tags.comment, color: "var(--ink2)" },
  { tag: tags.atom, color: "var(--ink3)" },
  { tag: tags.string, color: "var(--ok)" },
  { tag: tags.keyword, color: "var(--accent)", fontWeight: "600" },
]);

const errorLine = Decoration.line({ class: "cm-line-error" });
const markerLine = Decoration.line({ class: "cm-marker-line" });
const errors = new Compartment();

function errorAt(line: number | null): Extension {
  return EditorView.decorations.of((view) => {
    const b = new RangeSetBuilder<Decoration>();
    if (line && line <= view.state.doc.lines) b.add(view.state.doc.line(line).from, view.state.doc.line(line).from, errorLine);
    return b.finish();
  });
}

/** The first line of an LLM draft cannot be changed (design §6.5). */
function protectMarker(): Extension {
  return [
    EditorState.changeFilter.of((tr) => {
      const first = tr.startState.doc.line(1);
      if (!first.text.startsWith(MARKER)) return true;
      let ok = true;
      // Any change starting on line 1 (including its line break) is refused.
      tr.changes.iterChangedRanges((from) => {
        if (from <= first.to) ok = false;
      });
      return ok;
    }),
    EditorView.decorations.of((view) => {
      const b = new RangeSetBuilder<Decoration>();
      if (view.state.doc.line(1).text.startsWith(MARKER)) b.add(0, 0, markerLine);
      return b.finish();
    }),
  ];
}

const twoSpaces = keymap.of([
  { key: "Tab", run: (v) => (v.dispatch(v.state.replaceSelection("  ")), true), shift: indentLess },
]);

export function createEditor(parent: HTMLElement, text: string, generated: boolean, onChange: (text: string) => void) {
  return new EditorView({
    parent,
    state: EditorState.create({
      doc: text,
      extensions: [
        lineNumbers(),
        history(),
        twoSpaces,
        keymap.of([...defaultKeymap, ...historyKeymap]),
        EditorState.tabSize.of(2),
        language,
        syntaxHighlighting(highlight),
        errors.of(errorAt(null)),
        generated ? protectMarker() : [],
        EditorView.contentAttributes.of({ spellcheck: "false", autocorrect: "off", autocapitalize: "off", "aria-label": "草案原文" }),
        EditorView.updateListener.of((u) => u.docChanged && onChange(u.state.doc.toString())),
      ],
    }),
  });
}

export function setError(view: EditorView, line: number | null) {
  view.dispatch({ effects: errors.reconfigure(errorAt(line)) });
}

export function setText(view: EditorView, text: string) {
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
}
