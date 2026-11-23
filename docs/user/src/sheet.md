# Translation sheet

The translation sheet in the center shows the texts of the project, one row per text.

## Columns

| Column | Content |
|---|---|
| (first column) | Signs for the row: ✚ new, ✏ changed, ✖ unused, 🔒 do not translate, ★ marked. Point at it for a description. |
| Context | Where the text comes from, for example the file and the key. It tells equal texts apart. |
| Status | The status of the translation in the current language, see [Statuses](statuses.md). |
| Original | The text in the original language. |
| One column per language | The translations. A colored bar at the left shows the status, a small triangle at the top right a comment. |
| Row comment | A note on the row, for example from the developers. |
| Comment | The comment on the translation (only when one language is shown). |
| Translated on | When the translation of the current language was last changed. Point at a cell to see when the row was created and when its original changed. |

The original and the context have a lavender background. Cells with a problem found by the
[validation](validation.md) are pink. A camera in the context cell means that the row has a
[screenshot](screenshots.md).

The menu *View* switches the columns *Context*, *Row comment* and *Translated on* on and off.

## All languages or one language

The button *All languages* in the toolbar switches between two views:

- **All languages**: one column per language, side by side.
- **One language**: only the current language, with its status and comment. Under *View ›
  Additional languages* you can show further languages next to it, for example a related language
  as a reference.

The current language is selected in the toolbar, at the end of the menu *View*, or by clicking a
cell in a language column.

## Working with the sheet

- **Select** a row with a click or with the arrow keys. `Home` and `End` go to the first and last
  row.
- **Edit** a cell with `F2`, `Enter` or a double-click, see [Translating texts](translating.md).
- **Sort**: click the header *Context* to sort A–Z, Z–A or back to the original order. Click the
  header *Translated on* to sort newest first, oldest first or unsorted. An arrow marks the
  sorted column.
- **Column width**: drag the separator in the header. A double-click on the separator resets the
  width.
- **Context menu**: right-click a cell for the commands of the row, or a column header for the
  commands of the column, see [Copying and clearing translations](copy-and-clear.md).

## The bar above the sheet

| Button | Command |
|---|---|
| Status: ■ ■ ■ … | Sets the status of the selected cell. Point at a square to see its name. |
| ⏮ ◀ ▶ ⏭ | First, previous, next and last row |
| 🔒 | Switches *Do not translate* on or off for the row |
| ★ | Marks the row or removes the mark |
| ✏ | Opens the selected text in the [text editor](text-editor.md) |

At the end, the bar shows the number of rows in the current view.
