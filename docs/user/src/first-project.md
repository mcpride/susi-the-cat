# Your first project

This walk-through shows the whole way from the original texts of a program to the translated
files. Often a developer has already prepared the project; then you only need the steps from
"Translate" on.

## 1. Create a project

1. Choose *File › New project…* (`Ctrl+N`) and save the new project file (`.slp`) in the folder
   of the program, near the files with the texts.
2. The dialog *Languages* opens. Add the languages you want to translate into: choose a language
   in the list or type its code (for example `de` or `fr-CA`), and click *Add*. Then click
   *Close*.

## 2. Add the texts

1. Choose *Project › Add source…* and select the files that contain the original texts, for
   example `Resources.resx`, `en.json` or `messages.pot`.
2. Enter the original language, the language of the texts in these files, for example `en`, and
   click *Add*.

Susi reads the files immediately. Every text becomes a row in the translation sheet. The project
tree on the left shows the files and their structure.

## 3. Translate

1. Select the language you translate into in the toolbar (*Language:*).
2. Click a cell in the column of this language and press `Enter`, or double-click the cell.
   Type the translation and press `Enter`: Susi keeps the text and moves to the next row.
3. Repeat. The colored bar at the start of each cell shows the status of the translation, see
   [Statuses](statuses.md).

Tips:

- *Translation › Translate with translation memory…* fills in texts Susi already knows from
  earlier work, see [Translation memory](translation-memory.md).
- The filter bar on the right shows only the texts that still need work, for example only
  *Not translated*, see [Filter bar](filter.md).

## 4. Check

Choose *Validation › Validate*. The tab *Validation results* at the bottom lists possible
problems, for example a missing placeholder. Click an entry to jump to the cell. See
[Validation](validation.md).

## 5. Save and build

1. Save the project with *File › Save* (`Ctrl+S`).
2. Choose *Project › Build* (`F9`). Susi writes the translated files next to the original files,
   for example `Resources.de.resx` or `de/messages.po`. The tab *Output* lists every file
   written.

When the developers change the program and its texts, choose *Project › Scan for changes*
(`F5`). Susi adds new texts, marks changed ones and keeps all existing translations.
