/** The Note field (spec 7.1, variant A): a white card with a number pill, a textarea and a key legend. */
export interface NoteField {
  readonly card: HTMLElement;
  readonly textarea: HTMLTextAreaElement;
  /** Text is selected in the textarea, so Ctrl+C copies it rather than the PNG. */
  hasSelection(): boolean;
  size(): { width: number; height: number };
  place(left: number, top: number): void;
  /** The card's box changed (its width follows the image on screen): fit the textarea to its text again. */
  resize(): void;
  close(): void;
}

export interface NoteFieldOptions {
  /** The Box's number, shown in the pill. */
  number: number;
  /** The Note so far. */
  text: string;
  /** Card width in CSS pixels. */
  width: number;
  /** The text changed: typed, pasted or dictated. */
  onInput(text: string): void;
  /** The card's size may have changed. */
  onResize(): void;
}

function element<K extends keyof HTMLElementTagNameMap>(doc: Document, tag: K, className: string, text?: string) {
  const node = doc.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** Opens the card in `host`, focused, with the caret at the end of the text. */
export function openNoteField(doc: Document, host: HTMLElement, options: NoteFieldOptions): NoteField {
  const card = element(doc, "div", "note-card");
  card.style.width = `${options.width}px`;
  const head = element(doc, "div", "note-head");
  head.append(element(doc, "span", "note-num", String(options.number)), "Note");
  const textarea = element(doc, "textarea", "note-text");
  textarea.rows = 1;
  textarea.placeholder = "Type or dictate…";
  textarea.value = options.text;
  const foot = element(doc, "div", "note-foot");
  foot.append(element(doc, "span", "", "Enter save · Shift+Enter new line"), element(doc, "span", "", "Esc close"));
  card.append(head, textarea, foot);
  host.append(card);

  /** Grows with the text (dictation arrives all at once); the stylesheet caps it at about 40% of the window. */
  function grow(): void {
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  }
  textarea.addEventListener("input", () => {
    options.onInput(textarea.value);
    grow();
    options.onResize();
  });

  grow();
  textarea.focus();
  textarea.setSelectionRange(textarea.value.length, textarea.value.length);

  return {
    card,
    textarea,
    hasSelection: () => textarea.selectionStart !== textarea.selectionEnd,
    size: () => ({ width: card.offsetWidth || options.width, height: card.offsetHeight }),
    place(left, top) {
      card.style.left = `${left}px`;
      card.style.top = `${top}px`;
    },
    resize: grow,
    close: () => card.remove(),
  };
}
