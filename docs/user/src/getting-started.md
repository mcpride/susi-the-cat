# Installation and first start

## Installation

Susi needs no installation. Download the archive for your system from the
[releases page](https://github.com/mcpride/susi-the-cat/releases) and unpack it into a folder of
your choice:

| System | Archive |
|---|---|
| Windows | `susi-the-cat-<version>-windows-x64.zip` |
| Linux | `susi-the-cat-<version>-linux-x64.tar.gz` |
| macOS (Apple silicon) | `susi-the-cat-<version>-macos-arm64.tar.gz` |

The folder contains the desktop application `susi`, the command line program `susi-cli` and this
help in the folder `help`. Keep the files together: Susi opens the help from this folder.

On macOS, remove the download protection once: open the Terminal in the unpacked folder and enter
`xattr -d com.apple.quarantine susi susi-cli`.

## Starting Susi

Start `susi` (on Windows `susi.exe`). To open a project right away, drag the project file
(`.slp`) onto `susi.exe`, or let your system open `.slp` files with Susi.

On the first start, Susi fits its window to the screen. After that it remembers the size and
position of the window and the sizes of its panels, and restores them on the next start.

## Language of the user interface

Susi speaks English and German. By default it follows the language of your system. To change
it, choose *Tools › User interface language*. The change takes effect immediately.

## Getting help

- `F1` opens the help page for the part of the window you are working in: the open dialog, the
  tab of the output area, the project tree, the filter bar or the translation sheet. Point at
  the area with the mouse and press `F1`.
- *Help › User guide* opens the start page of the help.

The help opens in your web browser, in the language of the user interface. Susi uses the help
that came with your version. If it is missing, Susi opens the online help of the latest version.
