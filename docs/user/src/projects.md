# Creating, opening and saving

A Susi project is a file with the extension `.slp`. It refers to the source files with the
original texts and contains all translations, statuses and comments. Susi uses the same file
format as Sisulizer, so you can open existing Sisulizer projects.

## Opening a project

- *File › Open…* (`Ctrl+O`) opens a project file.
- *File › Recent projects* lists the projects you opened last.
- You can also start Susi with a project, for example by dragging the project file onto
  `susi.exe`.

## Creating a project

*File › New project…* (`Ctrl+N`) creates an empty project and asks for its file name. Save it in
or near the folder of the program to be translated: Susi stores the paths of the source files
relative to the project. Then add the [languages](languages.md) and the [sources](sources.md).
The walk-through [Your first project](first-project.md) shows all steps.

## Saving

- *File › Save* (`Ctrl+S`) saves the project. Before saving, Susi keeps the previous version as a
  backup copy `<name>.~slp` next to it.
- *File › Save as…* saves the project under a new name. Screenshots are copied along.

The status bar shows *Modified* while there are unsaved changes. When you close Susi, open or
create another project, Susi asks whether to save them: *Save*, *Don't save* or *Cancel*.

## Projects of Sisulizer

Susi keeps everything it does not know in a Sisulizer project when it saves it. Some kinds of
sources that Susi supports are unknown to Sisulizer 4, for example TSV catalogs: Sisulizer cannot
open a project that contains such a source.
