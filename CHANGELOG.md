# Changelog

## [0.4.0](https://github.com/mcpride/susi-the-cat/compare/v0.3.0...v0.4.0) (2026-11-09)

### Features

* **gui:** resizable, horizontally scrollable columns in bottom panel lists ([7492369](https://github.com/mcpride/susi-the-cat/commit/7492369a4c9b54962929ed1e38c8ab170b8246d0))

## [0.3.0](https://github.com/mcpride/susi-the-cat/compare/v0.2.1...v0.3.0) (2026-11-06)

### Features

* add Java resource bundles (.properties) support ([e847caf](https://github.com/mcpride/susi-the-cat/commit/e847caf32a4e6491262c6648a5aa3b6f33f2cf37))

## [0.2.1](https://github.com/mcpride/susi-the-cat/compare/v0.2.0...v0.2.1) (2026-11-04)

### Bug Fixes

* add testdata and build pipeline with automatic semver compatible versioning ([cd5c4c1](https://github.com/mcpride/susi-the-cat/commit/cd5c4c15bcfc6b9cd063af07d9c4322882bdaee3))

## 0.2.0 (2026-11-04)

### Features

* date filter and grid sorting ([5b215d6](https://github.com/mcpride/susi-the-cat/commit/5b215d6ef011eaed87b92813c79c95fd9f6d0b7c))
  * filter rows by creation, change or translation date (GUI "Date" filter, `-since`/`-until`/`-datetype` in `susi-cli`, `since`/`until`/`date_field` in the MCP tool `list_rows`)
  * "Translated on" column; clicking the "Translated on" and "Context" headers cycles through ascending, descending and unsorted

## 0.1.0 (2026-11-03)

### Features

* initial version ([4f9f5b0](https://github.com/mcpride/susi-the-cat/commit/4f9f5b0b5b2c9796d8b54b48d3ac88998f6f5e2d)): Sisulizer-compatible `.slp` projects, scan/build for .NET RESX, JSON, PO/POT and TSV sources, translation memory, Excel/CSV/XLIFF/TMX exchange, egui GUI, SlMake-compatible CLI and MCP server
