# Translation memory

The translation memory (TM) is a collection of texts you have translated before. Susi keeps it
in one file for all projects, so a translation you made once is offered again wherever the same
or a similar text appears.

## Suggestions while you translate

The [text editor](text-editor.md) shows suggestions from the translation memory for the selected
text, with their similarity in percent. *Apply* takes over a suggestion as the translation.

## Translating many texts at once

*Translation › Translate with translation memory…* (toolbar: 🗄 TM) fills in translations for
many rows:

| Option | Meaning |
|---|---|
| *Replace existing translations* | Also replaces translations that are already there. Without it, only empty cells are filled. |
| *Accept fuzzy matches from … % as "Best guess"* | Also takes over similar texts, with the status *Best guess*. |
| *Only rows of the current view* | Only the rows the sheet shows now. |
| *Only current language* | Only the current language instead of all languages. |

Exact matches get the status *Auto translated*. Check the results, for example with the filter
for *Auto translated* and *Best guess*.

## Filling the translation memory

- *Translation › Save to translation memory* adds the translations with the status *Translated*
  or *Complete* to the translation memory.
- In the [options](options.md), *Automatically save translated texts to the translation memory
  when saving* does this every time you save the project.

## Equal texts in one project

*Translation › Translate duplicate strings* fills empty cells with the translation of an equal
original elsewhere in the project, with the status *Auto translated*. This helps when the same
text, for example "Cancel", appears many times.

## Settings

In *Tools › Options…* you choose the file of the translation memory (TMX format) and the minimum
similarity for suggestions (*Minimum fuzzy match*).
