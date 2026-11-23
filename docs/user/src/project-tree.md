# Project tree

The project tree on the left shows the structure of the project:

- *All* at the top stands for the whole project. The number in brackets is the number of rows.
- Below it, there is one entry per source file, and inside it the parts of the file, for example
  the sections of a JSON file.

Click an entry to show only its rows in the translation sheet. Click *All* to show all rows
again. While a part is selected, the status bar shows *Filter active*. Click the small arrow in
front of an entry to expand or collapse it.

## Context menu

Right-click an entry for commands that apply to all of its rows:

| Command | What it does |
|---|---|
| *Status (current language)* | Sets the status of the translations in the current language. |
| *Do not translate* / *Allow translation* | Locks the rows against translation, or unlocks them. |
| *Clear new status* | Removes the mark *New* from the rows. |
| *Add screenshot to all rows…* | Links an image with all rows, see [Screenshots](screenshots.md). |
| *Statistics* | Shows the statistics. |
| *Validate* | Checks the translations. |

On a source file, there are more commands:

- *Properties…* opens the properties of the source, see [Sources](sources.md).
- *Scan for changes* and *Build* work on this file only.
- *Remove source…* removes the file with all its rows and translations from the project, after
  a confirmation. The file itself stays on the disk.

The entry *All* also offers *Add source…*, *Scan for changes* and *Build* for the whole project.
