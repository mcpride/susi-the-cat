# Copying and clearing translations

Sometimes a translation can start from an existing text, for example Swiss German (`de-CH`) from
German (`de`), or a product name that stays the same in all languages. Susi offers the same
commands for a single cell and for a whole column.

## For the selected cell

The menu *Row* (also *Edit* and the context menu of a cell) offers:

- *Status*: sets the status of the translation.
- *Copy from*: takes over the *Original* or the translation of another project language into the
  cell.
- *Clear translation*: deletes the translation of the cell.

## For a whole column

The menu *Column* works on the current language. A right-click on the header of a language or
status column offers the same commands for that language. They apply to all rows shown, so set a
filter first to limit them.

- *Status*: sets the status of all translations shown.
- *Copy from*: a dialog shows how many cells are empty and how many already have a translation.
  Empty cells are always filled. Check *Overwrite existing translations* to replace the existing
  ones as well.
- *Clear translation*: deletes the translations shown and their comments, after a confirmation.
  This cannot be undone, except by closing the project without saving.

Copying skips rows marked *Do not translate*, excluded rows and rows without a text to copy. A
copied translation gets its status like an edited one.
