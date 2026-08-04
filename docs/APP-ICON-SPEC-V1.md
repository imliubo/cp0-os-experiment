# CardputerZero App Icon Specification v1

The Apps screen has two user-selectable views. Pressing Tab at the Apps root
switches between List and Grid without changing the selected application.

## Grid geometry

- The viewport is 320x170 pixels.
- Grid mode contains four columns and two rows, for eight applications per
  page.
- Each cell is 72x57 pixels with a four-pixel horizontal gutter.
- The icon slot is exactly 40x40 pixels.
- Artwork must keep a two-pixel safe area, leaving a recommended 36x36 pixel
  visual footprint.
- The application label is one line and is clipped to ten ASCII glyphs in the
  compact grid. The full name remains available in List and App Detail.

## State and selection

The icon artwork does not encode process state.

- Running: the 40x40 icon-slot outline is green.
- Stopped, starting, or failed: the icon-slot outline is gray. Detailed state
  remains visible in List and App Detail.
- Selected: the cell uses a dark raised surface and a separate yellow outline.

This separation ensures that keyboard selection cannot be mistaken for an
application running in the background.

## Artwork sources

The eight system applications use built-in one-color procedural glyphs.
Applications without a system glyph receive an uppercase monogram derived from
their display name. A future manifest revision may add packaged raster artwork;
it must preserve the same 40x40 slot and state-outline behavior.
