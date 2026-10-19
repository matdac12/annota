/** The Editor's page: one canvas that draws the Snap. Empty until the Editor slices land. */
export function mountEditorPage(doc: Document): HTMLCanvasElement {
  const canvas = doc.querySelector<HTMLCanvasElement>("canvas#snap");
  if (!canvas) throw new Error("the Editor page has no canvas#snap");
  return canvas;
}
