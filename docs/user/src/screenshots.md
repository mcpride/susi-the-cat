# Screenshots

A screenshot shows where a text appears in the program, how much space it has and what it
refers to. A screenshot can be linked to one or more rows, optionally with the area where the
text appears.

The project must be saved before it can have screenshots. Susi keeps them in a folder next to the
project file (`<project>.files`), not in the project file itself.

## The tab Screenshots

The tab *Screenshots* at the bottom shows the screenshots of the selected row. The area of the
row is highlighted, the areas of other rows are shown faintly. If several screenshots are linked,
buttons above the image switch between them. A camera in the context cell of the sheet shows that
a row has a screenshot.

## Adding screenshots

- *Add…* in the tab adds image files (PNG or JPEG) and links them to the selected row.
- *Paste* or `Ctrl+V` pastes an image from the clipboard, for example a screenshot you just
  took.
- Drag image files from the file manager onto the Susi window.
- *Link existing…* links a screenshot that is already in the project.
- The menu *Row › Screenshot* adds an image to the selected row or to all rows of the view. In the
  project tree, *Add screenshot to all rows…* links it to all rows of a part.

## Marking the area of a text

1. Select the row and its screenshot.
2. Click *Mark area* and drag a rectangle over the place where the text appears.

*Clear area* removes the marking. *Remove link* removes the screenshot from the selected row; the
image stays in the project.

## Managing screenshots

*Project › Screenshots…* (or *Manage…* in the tab) lists all screenshots with title, size and
links. There you can change titles and remove screenshots.

- *Refresh* reads changed image files again, if a screenshot was taken from a file that has been
  replaced, for example by a new test run.
- *Clean up* removes links to rows that no longer exist and image files that are no longer used.
- *Import manifest…* (also *Project › Import screenshot manifest…*) takes over the screenshots
  of an automated UI test run, prepared by the developers.

Screenshots are included in exports, so external translators see them too, see
[Exchanging files](exchange.md).
