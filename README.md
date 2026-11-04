# Susi the CAT

**susi** ("Susi the CAT", CAT = computer-assisted translation) is a localization tool written in Rust. It is meant to replace the discontinued Sisulizer 4 and edits existing Sisulizer project files (`.slp`) directly.

The project consists of three parts:

| Program    | Purpose                                                                                |
|------------|----------------------------------------------------------------------------------------|
| `susi`     | Desktop GUI modeled on the Sisulizer user interface (tree, grid, filter, output)       |
| `susi-cli` | Command line for build pipelines, compatible with `SlMake`, also hosts the MCP server  |
| MCP        | `susi-cli mcp`: interface for AI agents (Model Context Protocol, stdio)                |

Supported sources are: 

* **.NET RESX** (`.resx`)
* **JSON** (i18next/ngx-translate key trees)
* **PO/gettext** (`.po`/`.pot`, including compilation to `.mo`)
* **TSV i18n catalogs** (`.tsv`, the format of Susi's own catalogs; not supported by Sisulizer).

---

## Building

Prerequisites:

- Rust ≥ 1.95 (edition 2024)
- On Windows: **Visual Studio Build Tools** with the "MSVC x64" component and the Windows SDK
  (x64 CRT). Without them the linker fails with `LNK1104`. If necessary, build from the
  "x64 Native Tools Command Prompt".

```powershell
cargo build --release          # target\release\susi.exe and susi-cli.exe
cargo xtask dist               # release build (with icon and version info) and copy to dist\
cargo xtask ci                 # rustfmt --check, clippy -D warnings, all tests
cargo xtask fmt | lint | test
cargo xtask icons              # render assets\app-icon\susi.svg to susi.ico and susi-256.png
cargo xtask set-version 0.3.0  # set the workspace version in Cargo.toml and Cargo.lock (used by the release)
```

Workspace layout:

```
crates/core   susi-core: project model (.slp read/write), RESX/JSON/PO scan and build, MO,
              translation memory (TMX), Excel/CSV/TXT/XLIFF/TMX exchange, validation,
              statistics, pseudo translation, filters, settings,
              i18n (user interface language, catalogs in crates/core/src/i18n/*.tsv)
crates/mcp    susi-mcp: MCP server (rmcp, stdio)
apps/susi     GUI (egui/eframe)
apps/susi-cli CLI (SlMake compatible) + `mcp` task
xtask         automation (cargo xtask …)
assets        app icon (see below)
testdata      test fixtures (see below)
.github       CI and release workflow; release tooling in package.json and .releaserc.json (see below)
```

### App icon (`assets/app-icon/`)

`susi.svg` is the master: a cat head shaped like a speech bubble (CAT = computer-assisted
translation) in front of a translation bubble. `cargo xtask icons` renders it to `susi.ico` (16–256 px) and `susi-256.png`;
both are committed, so a normal build needs no extra tools. `apps/*/build.rs` embed the icon and
version info into `susi.exe` and `susi-cli.exe` (via `rc.exe` from the Windows SDK; without it the
build only warns, `cargo xtask dist` fails). The GUI also uses the PNG as window icon and in the
About dialog.

### Test data (`testdata/`)

The integration tests run against synthetic fixtures that are part of the workspace:

| Directory | Content | Used by |
|-----------|---------|---------|
| `testdata/projects/demo-ui/`, `demo-settings/`, `demo-widget/` | Three JSON projects (`susi\demo.slp` and the JSON files under `assets\i18n`); `demo-ui` also references a source of `demo-widget` | scan/build tests (`crates/core/tests/ops.rs`), CLI test, headless GUI tests |
| `testdata/projects/wpf-resources/` | `Resources.slp`, `Resources.resx` and the German `Resources.de.resx` as built by Sisulizer | RESX build comparison |
| `testdata/slp/` | 193 further `.slp` projects of all source types (VCL, .NET, Java, Qt, HTML, databases, ...) | `.slp` round trip (`crates/core/tests/samples.rs`) |

- The fixtures are fully anonymized: every word in texts, translations, comments, keys, names and paths was
  replaced by a pseudo word of the same length, case pattern and script (Latin, Cyrillic, Greek, CJK, Hangul,
  Arabic, ...), consistently across all files, so projects and their sources still match. Binary
  data (images, icons, hashes) was replaced by random bytes. Only the file format structure remains:
  element and attribute names, class names, enumeration values, numbers, dates, escapes, whitespace
  and line endings.
- The `.slp` round trip covers all 197 projects. Projects written by Sisulizer 4.0.353 or newer
  must be reproduced byte for byte; for older ones the output must be stable.
- The tests work on copies in a temporary directory; `testdata/` itself is never modified.
- The JSON projects intentionally reproduce typical drift: `demo-ui` is in sync, `demo-settings`
  lacks two keys added to its source after the last scan, and `demo-widget` is an old project whose
  source has been rewritten since. The tests expect exactly these differences (as Sisulizer reports them).
- `.gitattributes` excludes `testdata/` from line-ending conversion, because the files are
  compared byte for byte.

### Continuous integration and releases

`.github/workflows/ci.yml` runs on every push and pull request:

1. **test** – `cargo xtask ci` (rustfmt, clippy, all tests) on Windows, Linux and macOS.
2. **semantic-release** (pushes to `main` only, after all tests passed) – [semantic-release](https://semantic-release.gitbook.io/)
   derives the next version from the commit messages since the last `v*` tag
   ([Conventional Commits](https://www.conventionalcommits.org/)):

   | Commit message                                   | Release |
   |--------------------------------------------------|---------|
   | `fix: …`, `perf: …`                               | patch (0.2.0 → 0.2.1) |
   | `feat: …`                                         | minor (0.2.0 → 0.3.0) |
   | `feat!: …` or a `BREAKING CHANGE:` footer          | major (0.2.0 → 1.0.0) |
   | `chore:`, `docs:`, `ci:`, `test:`, `refactor:`, `style:`, `build:` | no release |

   For a release it prepends the notes to `CHANGELOG.md`, sets the version in `Cargo.toml` and
   `Cargo.lock` (`cargo xtask set-version`), commits these files as `chore(release): x.y.z [skip ci]`,
   creates the tag `vx.y.z` and a draft GitHub release.
3. **build** – checks out the tag and runs `cargo xtask dist` on each platform; the archives
   `susi-the-cat-x.y.z-windows-x64.zip`, `…-linux-x64.tar.gz` and `…-macos-arm64.tar.gz` (with
   `susi`, `susi-cli`, `README.md`, `CHANGELOG.md` and a `.sha256` file each) are attached to the release.
4. **publish** – publishes the draft once all archives are attached.

Notes:

- The release tooling is pinned in `package.json`/`package-lock.json` (Node.js ≥ 24.10). Local
  preview of the next version: `npm ci`, then `npx semantic-release --dry-run --no-ci` (needs a
  `GITHUB_TOKEN` with access to the repository).
- The tag of the last release must exist (initially `v0.2.0`); without any `v*` tag semantic-release
  would start with 1.0.0.
- If `main` is a protected branch, the GitHub Actions bot must be allowed to push the release commit.
- The Linux and macOS builds are unsigned plain executables (no installer, no `.app` bundle). On macOS
  remove the quarantine flag after download: `xattr -d com.apple.quarantine susi susi-cli`.

---

## User interface (`susi`)

```powershell
susi.exe [project.slp]
```

Operation follows Sisulizer:

- **Menus**: File, Edit, Search, View, Project, Validation, Column, Row, Translation, Tools,
  Help. Keyboard shortcuts: `Ctrl+S`, `Ctrl+O`, `Ctrl+N`, `Ctrl+F`, `F5` (scan),
  `F9` (build).
- **Project tree** (left): "All", with the sources and their nodes below. The context menu offers
  properties, scanning/building a single source, setting the status of all rows of a node and
  "Remove source".
- **Translation grid** (center):
  - Columns for context, status, original and one column per language (or only the selected
    language with its comment). Context and original have a lavender background, problem cells
    a pink one.
  - "Translated on" shows when the translation of the selected language was last edited; its
    tooltip also shows when the row was created and when its original changed (*View* toggles
    the column).
  - Sorting: clicking the header of "Translated on" cycles through newest first → oldest first →
    unsorted (document order); rows without a translation date stay at the end. Clicking
    "Context" cycles through A–Z → Z–A → unsorted. An arrow marks the sorted column.
  - A colored bar shows the translation status, a triangle indicates a comment.
  - The grid scrolls horizontally and vertically; the header row stays fixed. Resize columns by
    dragging the separator in the column header; a double-click resets the width.
  - In-cell editing: `F2`/`Enter` or double-click; `Enter` accepts and moves to the next row,
    `Esc` cancels. Double-clicking multi-line text opens it in the Editor tab.
  - Editing changes the status *Not translated*, *Best guess* or *Auto translated* to
    *Translated*.
- **Filter** (right): translation status, row status (new, modified, unused, in use), date,
  marked, locked and text. When a filter is active, the notice "Special filters on!" appears.
  See [Reviewing recent changes](#reviewing-recent-changes).
- **Bottom tabs**:
  - Output, Validation (a click jumps to the cell), Search (including "Use as filter"),
    Statistics and Comments.
  - Editor: original, translation, comments, properties and translation memory suggestions with
    "Apply".
- **Dialogs**: Languages, Add source, Source properties (write method, output pattern),
  Row properties, Export, Import, Translate with translation memory, Pseudo translation, Options.

Unsaved changes are prompted for when closing, opening or creating a project.

### Reviewing recent changes

Like Sisulizer, Susi stores three timestamps (local time) in the `.slp` file:

| Timestamp | Set when | Date filter (`-datetype:`) |
|-----------|----------|----------------------------|
| Row created | a scan finds a new string | `created` |
| Original changed | a scan finds a changed original text | `changed` |
| Translation date (per language) | a translation is edited, imported, translated from the TM or pseudo translated | `translated` |

The date filter shows rows whose selected timestamp (or any of them) lies in a period:
*Today*, *Last 7 days*, *Last 30 days* or *Period…* with "From"/"To" (e.g. `2026-11-01`,
`01.11.2026 14:30`, `today`, `yesterday`, `7d`, `12h`; a date without time covers the whole day).
Translation dates are only checked for the shown languages (single-language view: the selected
one). The same filter is available in the CLI (`export`, `validate`, `report`) and in the MCP tool
`list_rows`:

```powershell
# Excel file with all German translations of the last 7 days for review
susi-cli export Review.xlsx -lang:de -since:7d -datetype:translated app.slp
# Strings that are new since a release
susi-cli export New.xlsx -since:2026-11-01 -datetype:created app.slp
```

Deleted translations and earlier texts leave no trace in the project; to compare with an older
state, compare the `.slp` file with its version in the source control.

---

## User interface language (English/German)

GUI and CLI are available in English and German. English is the default and fallback language.

- **GUI**: *Tools → User interface language* or *Tools → Options…*: "Automatic (system language)",
  "English" or "Deutsch". The change takes effect immediately and is stored in `settings.json`
  (`ui_language`). Automatic means: `SUSI_LANG`, otherwise the system language, otherwise English.
- **CLI**: precedence is `-uilang:de|en|auto` (also `--uilang=de`) → environment variable
  `SUSI_LANG` → GUI setting → system language → English. For build pipelines a fixed language is
  recommended, e.g. `-uilang:en`.
- **MCP** always answers in English.
- **Exchange files** (Excel/CSV) get column headers and status names in the user interface
  language. On import, both English and German headers and status names are recognized.

Texts are written in English in the code and serve as keys. The catalogs
`crates/core/src/i18n/de-*.tsv` (`core`, `cli`, `gui`) contain one entry per line:
`context<TAB>English<TAB>translation`. If an entry is missing, the English text is shown.
`cargo test -p susi-core i18n` checks that every translatable text has an entry, that no entry is
orphaned and that the `{placeholders}` match.

To add another language, create `xx-*.tsv` and register it in `LANGUAGES` in
`crates/core/src/i18n.rs`.

### Translating Susi

Susi maintains its own catalogs: `crates/core/src/i18n/susi-i18n.slp` has the three German
catalogs as TSV sources (original language English, see [TSV catalogs](#tsv-catalogs)).
`cargo test -p susi-core i18n` checks that the project covers all catalogs and that building it
reproduces them byte for byte.

- **New or changed texts**: add/change the line in `de-*.tsv` (with or without German translation),
  then `susi-cli scan crates\core\src\i18n\susi-i18n.slp`. The scan takes over German translations
  into empty cells. Translate the remaining rows in Susi and build (*Project → Build* or
  `susi-cli create crates\core\src\i18n\susi-i18n.slp`).
- **Changed translations**: edit them in Susi and build; the project is authoritative.
- **New language** (e.g. French): add it to the project and build. The output pattern
  `<iso>-core.<ext>` creates `fr-core.tsv` etc.; register the files in `LANGUAGES`.

---

## Command line (`susi-cli`, SlMake compatible)

```
susi-cli <tasks> project.slp [-q] [-a] [-w] [-e] [-h]
```

| Task | SlMake | Description |
|------|--------|-------------|
| `add [file]` | – | Add a source (.json/.resx/.po/.pot/.tsv, wildcards allowed); creates the project if needed. `-type:json\|resx\|po\|tsv` forces the type |
| `scan` | `scan` | Scan sources |
| `create` (`build`, `make`) | `create` | Create localized files (default task) |
| `export [file]` | `export` | Export to `.xlsx`, `.csv`, `.txt`, `.xlf`, `.tmx` |
| `import [file]` | `import` | Import from the same formats |
| `translate` | `translate` | Translate using the translation memory |
| `savetotm` | – | Save translations to the TM |
| `duplicate` | `duplicate` | Translate duplicate strings |
| `validate` | `validate` | Validate translations |
| `report [file]` | `report` | Report (`.html`, `.txt`, `.json`, otherwise stdout) |
| `remove` | `remove` | Remove unused rows |
| `fillpseudo` / `removepseudo` | same | Pseudo translation |
| `info` | – | Languages, sources, progress |
| `mcp` | – | Start the MCP server |

Important options are `-lang:de;fr`, `-status:…`, `-include:` and `-exclude:` (sources),
`-since:`, `-until:` and `-datetype:` (date filter, see
[Reviewing recent changes](#reviewing-recent-changes)),
`-overwrite`, `-tm:[file]`, `-fuzzy:N`, `-nosave` and `-forcesave`. Instead of `-opt:value`,
`--opt=value` is accepted as well. `-uilang:en|de` selects the output language. `susi-cli <task> -h`
prints help for a task.

**Exit codes**:

- With `-w`: `2` on errors, `1` on warnings, otherwise `0`.
- Without `-w`: `1` on errors, otherwise `0`.

Pipeline example:

```powershell
susi-cli scan create -w app.slp
susi-cli validate -lang:de -w app.slp
susi-cli export open.xlsx -lang:de;fr -status:untranslated app.slp
```

SlMake options without an equivalent in susi (e.g. `-ignorecase`) are ignored with a notice.
Unsupported tasks and translation engines (`-engine:` other than `tm`) abort with an error
message.

---

## MCP interface for AI agents

Configuration, e.g. in VS Code `mcp.json` or in the Copilot CLI:

```json
{ "servers": { "susi": { "command": "susi-cli", "args": ["mcp"] } } }
```

Optionally a project is opened right away: `"args": ["mcp", "C:\\path\\app.slp"]`.

Tools (21):

- `open_project`, `create_project`, `save_project`, `project_info`
- `list_rows` (filters including the date range `since`/`until`/`date_field`, paging; rows carry
  their created/changed and translation dates), `get_row`
- `set_translations` (batch; default status *Auto translated*), `set_status`,
  `set_row_properties`
- `add_language`, `add_source`, `scan`, `build`
- `validate`, `statistics`
- `tm_translate`, `tm_lookup`, `tm_save`
- `export`, `import`, `fill_pseudo`

A typical agent workflow:

1. `open_project`
2. `list_rows` with status "untranslated"
3. `set_translations`
4. `validate`
5. `save_project`

A human then reviews the translations in the GUI and sets them to *Translated* or *Complete*.

---

## Files and formats

- **Project**: Susi/Sisulizer `.slp` (XML). Unknown elements and attributes are preserved on save.
  A backup `*.~slp` is created before saving.
- **Output**:
  - RESX by default `<body>.<net>.<ext>` (e.g. `Strings.de-DE.resx`), JSON `<sl>\<file>`,
    PO `<iso>\<body>.po` (e.g. `de\messages.po`, with `messages.mo` next to it), TSV: the language
    code in the file name is replaced (`de-core.tsv` → `<iso>-core.<ext>`), otherwise `<iso>\<file>`.
  - Patterns from the project's `<outputfile>` are evaluated: `<dir>`, `<parent>`, `<file>`,
    `<body>`, `<ext>`, `<sl>`, `<iso>`, `<ish>`, `<net>`, `<tag>`, `<win>`, `<lang>`.
- **Translation memory**: TMX, by default `%APPDATA%\susi\susi-tm.tmx` (configurable). Exact
  matches get the status *Auto translated*, fuzzy matches *Best guess*.
- **Settings**: `%APPDATA%\susi\settings.json`. The directory can be overridden with
  `SUSI_CONFIG_DIR`.
- **Exchange**: Excel `.xlsx`, `.csv`, tab-separated `.txt`, XLIFF 1.2 (`.xlf`, one file per
  language when several languages are exported) and `.tmx`.

---

## PO localization (gettext)

A `.pot` template or a `.po` file is added like any other source (GUI: *Add source*, CLI:
`susi-cli add locale\messages.pot -lang:de;fr app.slp`).

- **Scan**: Each entry becomes a row. The row ID is `msgid` or `msgctxt|msgid`. Plural forms
  appear as `id[0]`, `id[1]`, … (count configurable, default 3, maximum 6). Comments (`#`, `#.`,
  `#:`) are imported as notes. The header fields `Last-Translator` and `Language-Team` are
  translatable as well.
- **Build**: The localized `.po` keeps comments, references and order. `Language`,
  `Content-Type` (UTF-8), `Plural-Forms` (matching the target language) and `PO-Revision-Date` are
  added or updated. Rows with status *For review* or *Best guess* get the `fuzzy` flag.
  Untranslated entries stay empty (gettext then falls back to the original).
- **MO**: With the option *Compile created PO files to binary MO files*, an additional `.mo` file
  is created. Fuzzy entries are skipped, as are incomplete entries unless *Include empty
  translations into MO files* is set. The byte order is selectable.
- **Options** (source properties, *PO options* section): scan comments, compile, use msgstr value
  as original, include empty translations, byte order, number of plural rows. The options are
  stored under the Sisulizer attribute names (`compile`, `scancomments`, `usevalueasoriginal`,
  `includeempty`, `byteorder`, `pluralforms`).

Note: Sisulizer's row ID scheme for PO is unknown. Older Sisulizer projects with PO sources may
therefore get new row IDs on the first scan. This has not been verified for lack of sample
projects.

---

## TSV catalogs

Tab-separated catalogs as used by Susi and PhotoCraft: one entry per line,
`context<TAB>original<TAB>translation`, comments start with `#`, escapes `\n`, `\t`, `\r`, `\\`.
The context `@plural` marks plural entries (`one|other`, translation forms separated by `|`); other
contexts disambiguate equal texts. Sisulizer has no such source type, so the project stores them
with the Susi-specific class `TTsvSource` (Sisulizer 4 cannot open such projects).

- **Scan**: Each entry becomes a row; the row ID is the original as written in the file, prefixed
  by `context|` if there is a context. The comment block heading a section and the context become
  the note. A catalog often already is the translation of one language (`de-core.tsv`: English
  originals, German translations): the scan takes over existing translations from the localized
  files of all project languages, including the original file itself, into empty cells.
- **Build**: Only the third column is rewritten; comments, blank lines, order, BOM and line endings
  are kept. Untranslated entries keep an empty third column, which the Susi catalog loader treats
  as "show the original". The localized file of a language may be the original file itself.

---

## Limitations (as of 0.1)

- Sources: RESX, JSON, PO and TSV. Other Sisulizer source types are preserved in the project but are
  neither scanned nor built.
- No Sisulizer exchange packages (`.sle`/`.slz`) and no merging of projects.
- No machine translation services. The MCP interface takes over this role.
- No visual form editor.

## ATTENTION

Before production use, do a trial run with copies of the real projects!
