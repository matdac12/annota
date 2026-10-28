import { expect, it } from "vitest";
import { Interaction, type Scene } from "../src/interaction";

/** A 400×300 Snap on a 1:1 screen (one CSS pixel per image pixel), so the 6 px rules read directly. */
function setup(cssPerImage = 1) {
  const scene: Scene = { width: 400, height: 300, boxes: [] };
  const interaction = new Interaction(scene, () => cssPerImage);
  const drag = (from: [number, number], to: [number, number]) => {
    interaction.pointerDown({ x: from[0], y: from[1] });
    interaction.pointerMove({ x: to[0], y: to[1] });
    return interaction.pointerUp();
  };
  return { scene, interaction, drag };
}

it("draws a Box by dragging on empty canvas, whichever way, and opens its Note field", () => {
  const { scene, interaction, drag } = setup();
  expect(drag([100, 100], [40, 60])).toBe(0);
  expect(scene.boxes).toEqual([{ x: 40, y: 60, w: 60, h: 40, note: "" }]);
  expect(interaction.selected).toBe(0);
  expect(interaction.preview).toBeNull();
});

it("shows the dashed preview while the button is down", () => {
  const { interaction } = setup();
  interaction.pointerDown({ x: 10, y: 10 });
  interaction.pointerMove({ x: 50, y: 30 });
  expect(interaction.preview).toEqual({ x: 10, y: 10, w: 40, h: 20 });
});

it("ignores a drag under 6 CSS px, measured on screen rather than in the image", () => {
  const { scene, drag } = setup();
  expect(drag([100, 100], [105, 140])).toBeNull();
  expect(scene.boxes).toHaveLength(0);

  const zoomedOut = setup(0.5);
  expect(zoomedOut.drag([100, 100], [110, 120])).toBeNull();
  expect(zoomedOut.drag([100, 100], [112, 112])).toBe(0);
});

it("selects a Box on click and clears the selection on empty canvas", () => {
  const { interaction, drag } = setup();
  drag([100, 100], [200, 200]);
  interaction.pointerDown({ x: 300, y: 50 });
  interaction.pointerUp();
  expect(interaction.selected).toBe(-1);
  interaction.pointerDown({ x: 150, y: 150 });
  interaction.pointerUp();
  expect(interaction.selected).toBe(0);
});

it("moves a selected Box by dragging inside it, clamped to the image", () => {
  const { scene, interaction, drag } = setup();
  drag([100, 100], [200, 200]);
  drag([150, 150], [170, 140]);
  expect(scene.boxes[0]).toMatchObject({ x: 120, y: 90, w: 100, h: 100 });
  drag([150, 150], [1000, 1000]);
  expect(scene.boxes[0]).toMatchObject({ x: 300, y: 200 });
  expect(scene.boxes).toHaveLength(1);
  expect(interaction.selected).toBe(0);
});

it("resizes from a corner handle of the selected Box, and a drag past the opposite corner flips it", () => {
  const { scene, drag } = setup();
  drag([100, 100], [200, 200]);
  drag([200, 200], [250, 230]);
  expect(scene.boxes[0]).toMatchObject({ x: 100, y: 100, w: 150, h: 130 });
  drag([250, 230], [60, 70]);
  expect(scene.boxes[0]).toMatchObject({ x: 60, y: 70, w: 40, h: 30 });
});

it("deletes the middle Box and the others renumber 1, 2", () => {
  const { scene, interaction, drag } = setup();
  drag([10, 10], [60, 60]);
  drag([100, 10], [160, 60]);
  drag([200, 10], [260, 60]);
  scene.boxes.forEach((box, index) => (box.note = `note ${index + 1}`));

  interaction.pointerDown({ x: 130, y: 30 });
  interaction.pointerUp();
  expect(interaction.removeSelected()).toBe(true);

  expect(scene.boxes.map((box) => box.note)).toEqual(["note 1", "note 3"]);
  expect(interaction.selected).toBe(-1);
});

it("deletes nothing without a selected Box", () => {
  const { scene, interaction, drag } = setup();
  drag([10, 10], [60, 60]);
  interaction.pointerDown({ x: 300, y: 250 });
  interaction.pointerUp();
  expect(interaction.removeSelected()).toBe(false);
  expect(scene.boxes).toHaveLength(1);
});

it("opens a Note field on double-click or Enter, for the Box under the point or the selected one", () => {
  const { interaction, drag } = setup();
  drag([10, 10], [60, 60]);
  drag([100, 10], [160, 60]);
  interaction.pointerDown({ x: 300, y: 250 });
  interaction.pointerUp();

  expect(interaction.editSelected()).toBeNull();
  expect(interaction.editAt({ x: 300, y: 250 })).toBeNull();
  expect(interaction.editAt({ x: 30, y: 30 })).toBe(0);
  expect(interaction.editing).toBe(0);
  expect(interaction.selected).toBe(0);
  interaction.stopEditing();
  expect(interaction.editSelected()).toBe(0);
});

it("picks the cursor: crosshair on empty canvas, move over a Box, resize over a selected corner", () => {
  const { interaction, drag } = setup();
  drag([100, 100], [200, 200]);
  expect(interaction.cursorAt({ x: 10, y: 10 })).toBe("crosshair");
  expect(interaction.cursorAt({ x: 150, y: 150 })).toBe("move");
  expect(interaction.cursorAt({ x: 200, y: 200 })).toBe("nwse-resize");
  expect(interaction.cursorAt({ x: 200, y: 100 })).toBe("nesw-resize");
});

it("puts a Box back as it was when a resize leaves it under 6 CSS px", () => {
  const { scene, drag } = setup();
  drag([100, 100], [200, 200]);
  drag([200, 200], [102, 103]);
  expect(scene.boxes[0]).toMatchObject({ x: 100, y: 100, w: 100, h: 100 });
});

it("selects a Box when its Callout is clicked, without dragging it, and edits it on double-click", () => {
  const scene: Scene = { width: 400, height: 300, boxes: [{ x: 50, y: 50, w: 40, h: 40, note: "look" }] };
  const interaction = new Interaction(scene, () => 1, () => [{ x: 50, y: 95, w: 100, h: 30 }]);
  interaction.pointerDown({ x: 100, y: 110 });
  expect(interaction.selected).toBe(0);
  expect(interaction.dragging).toBe(false);
  expect(interaction.cursorAt({ x: 100, y: 110 })).toBe("pointer");
  expect(interaction.editAt({ x: 100, y: 110 })).toBe(0);
  expect(scene.boxes[0]).toEqual({ x: 50, y: 50, w: 40, h: 40, note: "look" });
});

it("lays the Callouts out again only when a Box, a Note or the editing Box changes", () => {
  const scene: Scene = { width: 400, height: 300, boxes: [{ x: 50, y: 50, w: 40, h: 40, note: "look" }] };
  let laidOut = 0;
  const interaction = new Interaction(scene, () => 1, () => {
    laidOut += 1;
    return [{ x: 50, y: 95, w: 100, h: 30 }];
  });

  // Two moves over the same Boxes: nothing changed, so nothing is laid out again.
  interaction.cursorAt({ x: 10, y: 10 });
  interaction.cursorAt({ x: 20, y: 10 });
  expect(laidOut).toBe(1);

  interaction.startEditing(0);
  interaction.cursorAt({ x: 10, y: 10 });
  expect(laidOut, "the editing Box changed").toBe(2);
  interaction.stopEditing();

  scene.boxes[0].note = "look again";
  interaction.cursorAt({ x: 10, y: 10 });
  expect(laidOut, "a Note changed").toBe(3);

  // A Box moved by a drag is laid out once the drag is over, not at every pointer move.
  interaction.pointerDown({ x: 60, y: 60 });
  interaction.pointerMove({ x: 70, y: 70 });
  interaction.pointerMove({ x: 80, y: 80 });
  interaction.pointerUp();
  expect(laidOut, "no layout while the Box moves").toBe(3);

  interaction.cursorAt({ x: 95, y: 95 });
  expect(laidOut, "the Box moved").toBe(4);
});
