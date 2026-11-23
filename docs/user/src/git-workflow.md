# Git workflow

Many programs are developed with git, on a server such as GitLab or GitHub. Susi can exchange your
translations with this server for you, without any git knowledge. A developer prepares this once;
afterwards Susi shows the menu *Workflow* and three buttons in the toolbar.

## The three steps

| Step | Command | What happens |
|---|---|---|
| 1 | 📥 *Get translation project…* | Fetches the latest version of the project from the server and opens it. |
| 2 | 📤 *Save changes…* | Builds the translated files, saves the project and sends your changes to the server, into a branch of your own. |
| 3 | 🚀 *Prepare publication…* | Asks a reviewer to check your changes and take them over (a merge request on GitLab, a pull request on GitHub). |

You can save your changes as often as you like, also after preparing the publication: the
reviewer then sees the latest state.

## Getting the translation project

*Workflow › Get translation project…* replaces your local copy with the latest version from the
server. If you have changes that are not saved on the server yet, Susi warns you: they are set
aside and can only be restored by a developer. Save them first if you want to keep them.

## Saving changes

*Workflow › Save changes…* shows the branch and the changed files. Choose the *Kind of change*:

- *New translations (feat)* for new texts,
- *Corrected translations (fix)* for corrections,
- *Maintenance (chore)* for anything else.

The first time you save, Susi creates a personal branch for your changes. Only the project and its
translated files are sent, never other files.

## Preparing the publication

*Workflow › Prepare publication…* creates the merge or pull request. Enter a title and a
description and choose the reviewers. When the request is created, *Workflow › Open merge request
in the browser* (or *pull request*) shows it on the server.

## The state in the status bar

| State | Meaning and next step |
|---|---|
| *Up to date* | Translate, then save your changes. |
| *Update available* | A newer version is on the server: get the translation project. |
| *Changes not saved on the server* | Save your changes. |
| *Saved, not published* | When you are done, prepare the publication. |
| *In review* | The reviewer checks your changes. You can still save further changes. |
| *Published* | Your changes were accepted. Get the translation project to continue with the latest version. |
| *Fixed version* | An older version is shown for reading. Get the translation project to continue working. |

Click the state to ask the server for the latest information (*Workflow › Refresh state* does the
same).

## Signing in

The server needs to know who you are. *Workflow › Sign in to the server…* stores an access token,
a kind of password for programs, which you create on the server:

- **GitLab**: a personal access token with the scope `api`.
- **GitHub**: a fine-grained personal access token for the repository with the permissions
  *Contents* and *Pull requests* (read and write).

Your developers tell you which server to use and can help you create the token. Susi keeps it in
the credential store of the operating system, never in the project.

*Advanced settings* in the dialogs allow other branches or versions. You normally do not need
them.
