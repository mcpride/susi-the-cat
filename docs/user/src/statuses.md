# Statuses

Susi keeps two kinds of status: one for each translation and one for each row.

## Translation status

Every translation has a status. It is shown as a colored bar in the cell and in the column
*Status*.

| Status | Meaning |
|---|---|
| *Not translated* | There is no translation yet. |
| *Best guess* | A proposal, for example a similar text from the translation memory. It needs to be checked. |
| *Auto translated* | Filled in automatically, for example an exact match from the translation memory. |
| *Translated* | Translated by a person. |
| *For review* | Needs a second look, for example because the original has changed since it was translated. |
| *Complete* | Checked and final. |

How the status changes by itself:

- Editing a translation with the status *Not translated*, *Best guess* or *Auto translated* sets
  it to *Translated*.
- When a scan finds a changed original, the existing translations of the row get the status *For
  review*.
- The translation memory sets *Auto translated* for exact matches and *Best guess* for similar
  texts.

## Row status

The row status tells what a scan found out about the text:

| Status | Sign | Meaning |
|---|---|---|
| *New* | ✚ | The text was added with the last scans. |
| *Changed* | ✏ | The original has changed. |
| *Unused* | ✖ | The text is no longer in the source file. |
| *In use* | | The text is unchanged. |

*Project › Clear new status (all rows)* removes the mark *New*, for example when you have
translated all new texts. *Project › Remove unused rows* deletes the rows that are no longer used,
together with their translations.
