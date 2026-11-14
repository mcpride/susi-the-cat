# Changelog

## [0.5.0](https://github.com/mcpride/susi-the-cat/compare/v0.4.0...v0.5.0) (2026-11-14)

### Features

* implement actions for a simple, non-technical git workflow (gitlab target) ([7104196](https://github.com/mcpride/susi-the-cat/commit/710419640d9618e4e2f9ca491bdd24f535c977f9))
* implement actions for a simple, non-technical git workflow (gitlab target) ([dbe2bd6](https://github.com/mcpride/susi-the-cat/commit/dbe2bd61855dbfbd77ad4a54463acfae1cd1cf38))

### Bug Fixes

* keep workflow configuration when getting the translation project ([ea4d533](https://github.com/mcpride/susi-the-cat/commit/ea4d533e25898ad07a2e4359985102e3bd8d3a78))
* remember merge request state across further saves ([55710ce](https://github.com/mcpride/susi-the-cat/commit/55710ce736a7db7de325baf98007a9491183e129))

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
