# Filter bar

The filter bar on the right decides which rows the translation sheet shows. A row is shown when
it matches all settings. As soon as a filter is set, the bar shows *Special filters on!* and the
status bar *Filter active*. *Reset filter* (also in the menu *Search*) shows all rows again.

## Translation status

Shows only rows with a translation that has one of the checked statuses in one of the languages
shown. When only one language is shown, this is the current language. For example, check only
*Not translated* and *Best guess* to see what still needs work. See [Statuses](statuses.md).

## Row status

Shows rows by their state since the last scan: *New*, *Changed*, *Unused* or *In use*. Use *New*
and *Changed* to find the texts that came with a new version of the program.

## Date

Shows rows by a point in time: *Today*, *Last 7 days*, *Last 30 days* or *Period…* with *From*
and *To*. The second list chooses which time counts:

- *Any change*: any of the following.
- *Row created*: when a scan found the text for the first time.
- *Original changed*: when a scan found a changed original.
- *Translated*: when the translation was last edited, imported or filled in.

*From* and *To* accept, for example, `2026-11-01`, `01.11.2026 14:30`, `today`, `yesterday`, `7d`
(the last seven days) or `12h` (the last twelve hours). A date without a time covers the whole
day.

This is useful to review the work of the last days, or to find the texts that are new since the
last release.

## More

- *Only marked rows*: rows you marked with ★.
- *Only "Do not translate"*: rows that are locked against translation.
- *Show excluded*: also shows rows that are excluded from the translation.
- *Screenshot*: only rows with or without a screenshot.

## Text

Shows only rows that contain the text. The list below chooses where to search (all columns,
original, translation, context or comment), and *Match case* distinguishes upper and lower case.
To search without hiding rows, use [Search](search.md).
