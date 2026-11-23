# Sources

A source is a file with the original texts of the program, for example a resource file of the
developers. Susi reads the texts from the sources (*scan*) and writes translated copies of them
(*build*).

## Supported files

| Kind | Files |
|---|---|
| .NET resources | `.resx`, `.resw` (including pictures and other data in `.resx`) |
| JSON | key trees as used by i18next or ngx-translate, and `messages.json` of browser extensions |
| gettext | `.po`, `.pot` |
| i18n catalogs | `.tsv` |
| Java | `.properties` |
| Android | `res/values/strings.xml` |
| Flutter | `.arb` |
| Qt Linguist | `.ts` |

## Adding a source

1. Choose *Project › Add source…* (toolbar: ➕ Source) and select one or more files.
2. Enter the *Original language* of the files, for example `en`.
3. Click *Add*.

Susi stores the files relative to the project and reads them immediately.

## Scanning

*Project › Scan for changes* (`F5`, toolbar: ⟳ Scan) reads all sources again, for example after
the developers changed the program:

- New texts are added with the row status *New*.
- Changed texts get the row status *Changed*, and their translations *For review*.
- Texts that are no longer in the file get the row status *Unused*. Their translations are kept,
  in case the text comes back.

The tab *Output* shows a summary. *Project › Remove unused rows* deletes the unused rows when they
are no longer needed.

## Building

*Project › Build* (`F9`, toolbar: 🔨 Build) writes one translated file per source and language,
for example `Resources.de.resx`, `de/messages.po` or `res/values-b+de/strings.xml`. The tab
*Output* lists every file written. Texts that are not translated get the original text or are
left out, depending on the kind of file, so the program shows the original instead.

To scan or build a single source, use the context menu of the source in the project tree.

## Source properties

Right-click the source in the project tree and choose *Properties…*:

| Setting | Meaning |
|---|---|
| *Original language* | The language of the texts in the file. |
| *Localized files contain* | *Different*: only texts whose translation differs from the original. *Translated*: only translated texts. *All*: all texts. |
| *Output directory* and *File pattern* | Where the translated files are written and how they are named. Placeholders such as `<iso>` (language code), `<file>` (file name) or `<body>` (name without extension) are replaced. |
| *Encoding* | For Java `.properties` files: the character encoding of the translated files. |
| *PO options* | For gettext files: read comments, create `.mo` files and more. |
| *Excluded data types* | For `.resx` files: pictures, binary data or text files that are not to be translated. |

Usually the developers set these up once. Ask them before you change the output settings: the
program expects the translated files in a certain place.
