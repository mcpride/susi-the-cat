# Validation

The validation checks the translations for typical mistakes before they reach the program.

## Running a validation

- *Validation › Validate* (toolbar: ✔ Validate) checks all translations.
- *Validation › Validate (including missing translations)* also reports texts that are not
  translated yet.
- *Validation › Clear validation results* removes the results.

The tab *Validation results* lists every finding with its type (*Error* or *Warning*), the check,
the language, the context and a message. Click an entry to jump to the cell. Cells with a finding
are pink in the translation sheet.

## The checks

| Check | Finds |
|---|---|
| *Missing translation* | Texts without a translation (only with *including missing translations*). |
| *Inconsistent format string* | Placeholders such as `{0}`, `%s` or `{name}` that differ from the original. |
| *Inconsistent tags* | Markup such as `<b>` that differs from the original. |
| *Invalid number of spaces* | Different leading or trailing spaces. |
| *Invalid number of line breaks* | A different number of line breaks. |
| *Missing period*, *colon*, *exclamation mark*, *question mark* | The original ends with the sign, the translation does not. |
| *Length in characters* | Translations longer than the maximum of the row (*Max. characters*). |
| *Translation consists of spaces only* | Translations without visible text. |
| *Invalid number of hotkeys* | A different number of `&` hotkeys, as in `&File`. |
| *Duplicate translation* | The same original translated differently. |
| *Invalid control characters* | Invisible control characters. |
| *Invalid ICU message*, *Incomplete ICU cases* | Errors in messages with plural or select forms, for example missing plural forms of the language. |
| *Missing screenshot* | A linked screenshot whose image file is missing. |
| *Custom data* | Missing linked files, or translated pictures with another format or size than the original. |

Not every finding is a real mistake: a missing period can be right in another language. Decide
case by case.

## Excluding findings

If a check reports something that is correct, exclude the finding:

- Right-click the finding in *Validation results* and choose *Exclude*. This check is no longer
  reported for the row, in all languages. The other checks of the row still run.
- *Validation › Excluded original validations…* and *Validation › Excluded translation
  validations…* list the exclusions with check, context and original. *Remove* or *Remove all*
  brings the findings back, and the validation runs again.

Findings in the original text, such as control characters in the original, are listed under
*original validations*, all others under *translation validations*. If the original text of the
row changes, the exclusion no longer applies.

The exclusions are saved with the project, in the file `.sds` next to the project file. Sisulizer
uses the same file: findings excluded in Sisulizer are also excluded in Susi, and the other way
round. The command line (`susi-cli validate`) skips them as well.
