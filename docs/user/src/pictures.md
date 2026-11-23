# Pictures and binary data

Besides texts, a program can contain pictures and other data that differ per language, for example
a logo with text in it or a sound file. Susi shows such rows in the sheet with a short description,
for example `PNG 16 × 8` or *Binary data*. A 🔗 in front of it means that the translation is
linked to a file.

## Translating a picture

Select the row and open the [text editor](text-editor.md). For pictures and binary data, it shows
the original on the left and the translation on the right: pictures as images, other data as a
hex dump.

| Button | What it does |
|---|---|
| *Save as…* (left) | Saves the original, for example as a template for the translated picture. |
| *Load…* | Takes a file as the translation. |
| *Link to file…* | Takes a file as the translation and keeps it linked: when the file changes, Susi reads it again when the project is opened, scanned or built. |
| *Remove file link* | Keeps the translation, but no longer reads the file. |
| *Save as…* (right) | Saves the translation as a file. |
| *Clear translation* | Deletes the translation, so the program uses the original. |

The same commands are in the menu *Row › Picture or binary data*.

A linked file is the comfortable way when a designer delivers the translated pictures: replace
the file, and Susi takes over the new content.

## Checks

The [validation](validation.md) (check *Custom data*) reports linked files that are missing, and
translated pictures with another format or size than the original.
