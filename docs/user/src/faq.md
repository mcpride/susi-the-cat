# Questions and answers

## The sheet is empty or shows only a few rows. Where are my texts?

Probably a filter is active: the status bar then shows *Filter active*. Click *Reset filter* in the
filter bar (or *Search › Reset filter*) and *All* in the project tree.

## Why is a translation suddenly "For review"?

The original has changed since it was translated, and the scan marked the translation for a second
look. Check whether it still fits, correct it if necessary, and set the status to *Translated* or
*Complete*.

## A text must stay in English. How do I prevent it from being translated?

Select the row and choose *Row › Toggle do not translate* (or 🔒 above the sheet). The text then
stays as it is in all languages.

## I have translated everything, but the program still shows English texts.

The translated files are only written when you build: choose *Project › Build* (`F9`) and check
the tab *Output* for the files written. Then the program must be built or installed again by the
developers, with the new files.

## Can I undo a change?

There is no undo command. Correct the text again, or close the project without saving to discard
all changes since the last save. Before every save, Susi keeps the previous version as
`<name>.~slp`.

## Can Sisulizer still open my project?

Yes, as long as it contains only kinds of sources that Sisulizer knows. Susi keeps everything else
in the file untouched. See [Creating, opening and saving](projects.md).

## The menu *Workflow* and its buttons are missing.

Susi shows them only for projects that a developer has prepared for the
[git workflow](git-workflow.md). Look at the tab *Output* after opening the project: if git is not
installed, Susi says so there. Install Git for Windows with the option *Git from the command line
and also from 3rd-party software* and restart Susi. Without a message, the project is not in a
prepared working copy: ask your developers.

## Where does Susi keep my settings and the translation memory?

In the settings folder of your user account, see [Options](options.md).

## The help does not open.

Susi opens the help in your web browser: first the help that came with your version (folder
`help` next to `susi`), otherwise the online help. If neither opens, open the file
`help/en/index.html` (or `help/de/index.html`) by hand, or visit the
[online help](https://mcpride.github.io/susi-the-cat/).
