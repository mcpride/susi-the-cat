# Exchanging files

Not everybody who translates has Susi. You can export the texts into a file, send it to a
translator or an agency, and import the translated file again.

## Formats

| Format | Extension | Good for |
|---|---|---|
| Excel workbook | `.xlsx` | Translators who work in Excel; screenshots can be embedded. |
| CSV | `.csv` | Other spreadsheet programs (separated by semicolons). |
| Text | `.txt` | Simple tools (separated by tabs). |
| XLIFF 1.2 | `.xlf` | Translation tools of agencies (one file per language). |
| TMX | `.tmx` | Translation memories of other tools. |

## Exporting

1. Set a filter if you want to export only some rows, for example only *Not translated*.
2. Choose *File › Export (Excel, CSV, XLIFF, TMX)…* and enter a file name. The extension decides
   the format.
3. Choose the options and click *Export*:

| Option | Meaning |
|---|---|
| *Languages* | The languages to export. *All languages* above the list selects or clears all of them at once. |
| *Only rows of the current view* | Only the rows the sheet shows now, otherwise all rows. |
| *Status columns*, *Comment columns* | A status and a comment column for every language. |
| *Row status column*, *"Max. characters" column* | More information on the rows. |
| *Include "Do not translate" rows* | Also exports the rows that must not be translated. |
| *Language of column headers and status values* | The language of the column titles, for example English for an agency abroad. |
| *Screenshots* | Whether screenshots are embedded (Excel, on a second worksheet), copied as files next to the export with links, or left out. |

Tell the translators to keep the columns *Source*, *Context* and *Original* unchanged: Susi needs
them to find the rows again.

## Importing

Choose *File › Import…* and select the translated file. Then decide:

| Option | Meaning |
|---|---|
| *Overwrite existing translations* | Replaces translations that are already in the project. Without it, only empty cells are filled. |
| *Skip rows whose original has changed* | Ignores rows whose original has changed since the export, because the translation may no longer fit. |
| *Import comments* | Takes over the comments of the file. |
| *Status for imported texts without a status* | The status for translations that come without a status column. |

The tab *Output* shows how many translations were imported, kept or skipped. Column headers and
status values are recognized in every language of the user interface.
