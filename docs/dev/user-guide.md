# Maintaining the user guide

The user guide for translators and reviewers lives in [`docs/user`](../user/). It is written in
English as Markdown, built with [mdBook](https://rust-lang.github.io/mdBook/), and translated with
[mdbook-i18n-helpers](https://github.com/google/mdbook-i18n-helpers) (gettext). The translations
are maintained with Susi itself.

```
docs/user/
  book.toml              mdBook configuration
  src/                   English pages (SUMMARY.md: table of contents)
  po/messages.pot        English texts extracted from src/ (generated, committed)
  po/de.po               German translation (built by Susi from susi-help.slp)
  susi-help.slp          Susi project: source po/messages.pot, language de, output po/<iso>.po
  theme/                 language switcher (language.js) and styles (susi.css)
  language-select.html   start page of the online help (chooses the browser language)
```

## Tools

```powershell
cargo install mdbook --version 0.5.4 --locked
cargo install mdbook-i18n-helpers --version 0.4.0 --locked
```

The versions are pinned in `.github/workflows/ci.yml` as well; update both places together
(mdbook-i18n-helpers 0.4 requires mdBook 0.5).

## Building

```powershell
cargo xtask help            # updates po/messages.pot, builds target/help/en/, target/help/de/
cargo xtask help --check    # as in CI: fails if po/messages.pot was out of date
cargo xtask dist            # release build; the guide goes to dist/help/ (next to susi.exe)
```

`target/help/index.html` is the start page of the online help. A development build of `susi`
in `target/debug` finds the guide in `target/help` as well, so `F1` can be tried without a
release build. To preview while writing, run `mdbook serve docs/user` (English) or, for German,
`$env:MDBOOK_BOOK__LANGUAGE = "de"; mdbook serve docs/user`.

## Changing the English text

1. Edit the pages in `docs/user/src`. Write for translators, not developers: short sentences,
   menu paths in italics with `›` (*File › Open…*), keys as code (`Ctrl+S`), the exact labels of
   the user interface.
2. Link to pages, not to headings: the anchors of headings are derived from the translated
   text and differ per language.
3. Run `cargo xtask help` and commit `po/messages.pot` with the change.
4. Update the translation (next section). `cargo test -p susi` fails until `susi-help.slp` and
   `po/de.po` match the new `messages.pot`.

## Translating

1. Open `docs/user/susi-help.slp` in Susi and *Project › Scan for changes* (`F5`), or run
   `susi-cli scan docs/user/susi-help.slp`. New texts appear as *New*, changed ones are marked
   *For review*.
2. Translate. The row comment shows the page of a text (`src/sheet.md`). Keep Markdown as it is:
   `*italic*`, `**bold**`, `` `code` ``, links `[text](page.md)` with the target unchanged.
   Use the labels of the German user interface (`crates/core/src/i18n/de-gui.tsv`).
3. *Project › Build* (`F9`) or `susi-cli create docs/user/susi-help.slp` writes `po/de.po`.
   mdbook-gettext ignores entries marked *fuzzy* (status *For review* or *Best guess*): they
   appear in English until they are translated or confirmed.
4. Run `cargo xtask help`, check `target/help/de/`, and commit the project and `po/de.po`.

The test `help::tests::translations_match_the_susi_project` checks that the project is up to date
with `messages.pot` and reproduces `po/de.po`.

## Adding a language

1. Add the language to `susi-help.slp` (*Project › Languages…*), translate and build it:
   Susi writes `po/<iso>.po`.
2. Add it to `HELP_LANGUAGES` in `xtask/src/main.rs` (with the book title), to `languages` in
   `docs/user/theme/language.js`, to `language-select.html` and to `LANGUAGES` in
   `apps/susi/src/help.rs`.

## Context help (F1)

`apps/susi/src/help.rs` maps the open dialog, the tab of the output area or the panel under the
mouse pointer to a page (`topic`). The pages are file names in `docs/user/src` without `.md`; the
tests check that every page exists and is listed in `SUMMARY.md`. Susi opens
`help/<lang>/<page>.html` next to the executable (the guide of this version), otherwise the online
help `https://mcpride.github.io/susi-the-cat/<lang>/<page>.html` (the latest release).

## Publishing

The release workflow attaches the guide to every archive (`help/`) and deploys the guide of the
release to GitHub Pages (job *online help*). This requires *Settings › Pages › Source: GitHub
Actions* in the repository, once. The search of the online and offline help works per language:
every language is a book with its own search index.
