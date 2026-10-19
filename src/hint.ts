/** The hint line under the image: the only chrome (spec 6.4). */
export function renderHint(hint: HTMLElement, notes: number): void {
  const count = hint.ownerDocument.createElement("b");
  count.textContent = `${notes} ${notes === 1 ? "note" : "notes"}`;
  hint.replaceChildren(count, " · Caps+S / Ctrl+C copy · Esc discard");
}
