/**
 * Marks written over the presentation (docs/design/tablet-live-classroom.md, "M8: writing over
 * the presentation"): temporary strokes in page units, kept by the presentation window only,
 * never saved into the note and gone when the page changes. Pure helpers; the window holds the
 * list and `LiveRenderer` draws it.
 */
import { polylinesDistanceSq, type Vec } from './editor/geometry';
import type { InkPoint, Tool } from './ink';
import { INKSPACE_COLORS, type Rgba, type Stroke } from './note';

/** Line diameter of a mark at full pressure, in screen pixels at the moment it is written. */
export const MARK_WIDTH_PX = 6;
/** Radius of the eraser on the marks, screen pixels. */
export const MARK_ERASER_PX = 18;
/** The ink of the marks: red on a light page, yellow on a dark one (blackboard, slate paper). */
const MARK_ON_LIGHT: Rgba = INKSPACE_COLORS[4];
const MARK_ON_DARK: Rgba = { r: 255, g: 214, b: 0, a: 255 };

export interface MarkStyle {
  tool: Tool;
  color: Rgba;
}

export function markStyle(darkPage: boolean): MarkStyle {
  return { tool: 'feltTip', color: darkPage ? MARK_ON_DARK : MARK_ON_LIGHT };
}

/** A new mark of the given style starting at `first`; `scale` is the screen pixels per page unit. */
export function newMark(style: MarkStyle, first: InkPoint, scale: number): Stroke {
  return {
    tool: style.tool,
    color: { ...style.color },
    width: MARK_WIDTH_PX / Math.max(1e-6, scale),
    layer: 0,
    points: [first],
  };
}

/**
 * The marks a path of the eraser leaves: those within `radius` (page units) of the path are
 * removed whole. The same array comes back when nothing was touched.
 */
export function eraseMarks(marks: readonly Stroke[], path: readonly Vec[], radius: number): readonly Stroke[] {
  const kept = marks.filter((mark) => {
    // The line's own half width counts too: a thick mark is touched by the edge of the circle.
    const reach = (radius + mark.width / 2) ** 2;
    return polylinesDistanceSq(path, mark.points) > reach;
  });
  return kept.length === marks.length ? marks : kept;
}
