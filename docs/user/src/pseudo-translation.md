# Pseudo translation

A pseudo translation is an artificial translation for testing. It shows early, before the real
translation, whether the program is ready for other languages: whether all texts come from the
resource files, whether longer texts fit into the dialogs and whether special characters are
displayed correctly.

## Creating a pseudo translation

*Translation › Create pseudo translation…* opens a dialog:

| Setting | Meaning |
|---|---|
| *Language* | The language that receives the pseudo translation. It is created as a pseudo language if necessary, for example `qps`. |
| *Kind* | *Emulation* changes the original as set below. *Replace* replaces every character by a fill character, *Reduce* shortens every text to its first character. *Longest translation* takes the longest text of the row in any language and changes it like *Emulation*. |
| *Diacritics* | Replaces letters by similar ones with accents: `Save` becomes `Šàvé`. |
| *Expand text* | Makes the texts longer, as many languages need more space than English. |
| *Enclose in delimiters* | Puts delimiters around every text, so cut-off texts are easy to spot. |
| *Case* | Leaves the case unchanged, alternates it or uses upper case. |
| *Overwrite existing translations* | Also replaces real translations of the language. Existing pseudo translations are always renewed. |

*Sample* shows the effect on an example text. Click *Create*, then build the project and start the
program with the pseudo language.

*Translation › Remove pseudo translations* deletes the pseudo translations again.
