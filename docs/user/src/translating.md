# Translating texts

## Editing in the sheet

1. Select the language you translate into in the toolbar.
2. Click the cell of the row in the column of this language.
3. Press `F2` or `Enter`, or double-click the cell, and type the translation.
4. Press `Enter` to keep the text and move to the next row, or `Esc` to cancel.

Texts with several lines are easier to edit in the [text editor](text-editor.md): a double-click
on a multi-line text opens it there.

When you edit a translation with the status *Not translated*, *Best guess* or *Auto translated*,
Susi sets it to *Translated*. When you are sure that a translation is final, set it to
*Complete*. See [Statuses](statuses.md).

## Setting the status

- Click one of the colored squares in the bar above the sheet, or
- right-click the cell and choose *Status*, or
- use the menu *Row › Status*.

To set the status of many rows at once, use the menu *Column › Status* (all rows shown), or the
context menu of the project tree (all rows of a part).

## Several rows at once

Select several rows with `Ctrl`+click (adds a row or removes it) or `Shift`+click (all rows from
the current row to the clicked one). `Ctrl+A` selects all rows of the sheet. The status, *Copy
from*, *Clear translations…*, the marks and *Screenshot › Add for the selected rows…* then apply
to all selected rows, in the current language:

- A mark (*Do not translate*, *Marked*, *Excluded*) takes its new value from the current row, the
  one with the frame: if it is not marked, all selected rows become marked.
- Clearing several translations asks first.
- *Properties…* and the commands for pictures concern one row only; they are not offered then.
- A click without `Ctrl` or `Shift`, or an arrow key, selects a single row again.

## Rows that need special care

Some rows must not be translated, for example technical names, or should be left out for now.
The menu *Row* and the bar above the sheet switch these marks:

| Mark | Meaning |
|---|---|
| *Do not translate* (🔒) | The text stays as it is in all languages. |
| *Marked* (★) | A personal reminder, for example "ask the developers". The filter bar can show only marked rows. |
| *Excluded* | The row is left out of the translation and hidden, unless the filter bar shows excluded rows. |

## Row properties

*Row › Properties…* (or *Edit › Row properties / comment…*) shows the context and the original of
the selected row and lets you change:

- the row status (*New*, *Changed*, *Unused*, *In use*),
- *Max. characters*: the maximum length of the translations. The [validation](validation.md)
  reports longer translations, and the text editor counts the characters,
- the row comment, for example a hint for other translators,
- the marks *Do not translate*, *Excluded* and *Marked*.

## Moving around

| Keys | Go to |
|---|---|
| `↓` / `↑` | Next / previous row |
| `Home` / `End` | First / last row |

The menu *Row* and the buttons ⏮ ◀ ▶ ⏭ above the sheet and in the text editor do the same.

## Saving

Your translations are part of the project file. Save it with *File › Save* (`Ctrl+S`). When you
close Susi or open another project with unsaved changes, Susi asks whether to save them.
