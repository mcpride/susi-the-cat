//! rmcp tool router exposing a [`Session`] over MCP.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::session::{ListRowsInput, ScreenshotKeyInput, Session, ToolResult, TranslationInput, WorkflowFetchInput, WorkflowPublishInput, WorkflowSaveInput};

const INSTRUCTIONS: &str = "Susi the CAT (Sisulizer compatible). Typical AI translation workflow: \
open_project(path) -> project_info -> list_rows(status=[\"untranslated\"], langs=[\"de\"]) -> \
set_translations([{key, lang, text}], status defaults to 'auto') -> validate -> save_project. \
Keep placeholders ({0}, {name}, %s, <tags>), leading/trailing spaces, line breaks and '&' hotkeys exactly as in the original; \
respect max_chars and row comments. Rows marked do_not_translate must not be translated. \
To fill a regional variant from a related language (e.g. de-CH from de) or a language from the original, \
use copy_translations instead of writing every text with set_translations. \
scan updates the project from the source files, build writes the localized files. \
If the project is in a git working copy configured for the translation workflow (.susi/workflow.toml): \
workflow_status -> workflow_fetch (latest version) -> translate -> workflow_save (commit + push to a personal branch) -> \
workflow_publish (GitLab merge request or GitHub pull request for review). \
Rows may have screenshots (list_rows/get_row: screenshots [{id, title, area}], area = [x, y, width, height] in pixels): \
if the meaning, tone or available space of a text is unclear, look at them with get_screenshot(id) before translating. \
Rows with `data` instead of text hold pictures or binary data (.resx images, byte arrays): get_picture shows a picture, \
load_translation_file sets a translated file; set_translations does not apply to them.";

#[derive(Clone)]
pub struct SusiServer {
    state: Arc<Mutex<Session>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathInput {
    /// Absolute path of the file.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct OptPathInput {
    /// Target path (default: the path the project was opened from).
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateInput {
    /// Path of the new .slp file.
    pub path: String,
    /// Language ids, e.g. ["en", "de", "fr"]; the original language of the sources should be included.
    pub languages: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RowKeyInput {
    /// Row key (context) as returned by list_rows.
    pub key: String,
    /// Source index (only needed if the key is ambiguous).
    pub source: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetTranslationsInput {
    /// Translations to write (batch).
    pub items: Vec<TranslationInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetStatusInput {
    /// Row keys.
    pub keys: Vec<String>,
    pub source: Option<usize>,
    /// Target language id.
    pub lang: String,
    /// untranslated|best|auto|translated|review|complete
    pub status: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CopyTranslationsInput {
    /// Language to copy from (default: the original text).
    pub from: Option<String>,
    /// Target language id.
    pub lang: String,
    /// Row keys (default: all rows, or all rows of `source`).
    #[serde(default)]
    pub keys: Vec<String>,
    /// Source index: limits the rows without keys, disambiguates keys.
    pub source: Option<usize>,
    /// Replace existing translations (default false: only empty cells are filled).
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RowPropsInput {
    pub key: String,
    pub source: Option<usize>,
    /// Row comment for translators (empty string removes it).
    pub comment: Option<String>,
    /// "Do not translate" flag.
    pub do_not_translate: Option<bool>,
    /// Maximum number of characters for translations (0 = unlimited).
    pub max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LangInput {
    /// Language id, e.g. "de", "fr-CA".
    pub lang: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddSourceInput {
    /// Path of a .json, .resx, .resw, .po, .pot, .tsv, .properties, .arb or Qt .ts file, an Android values file (res/values/strings.xml) or WebExtension messages (_locales/<lang>/messages.json).
    pub file: String,
    /// Language of the texts in the file (default "en").
    pub original_lang: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SourcesInput {
    /// Source indices (default: all).
    pub sources: Option<Vec<usize>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct BuildInput {
    /// Languages to build (default: all).
    #[serde(default)]
    pub langs: Vec<String>,
    pub sources: Option<Vec<usize>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ValidateInput {
    #[serde(default)]
    pub langs: Vec<String>,
    /// Checks: missing, format, tags, whitespace, newline, period, colon, exclamation, question, length, allspaces, hotkey, duplicate, control, icu, icucases (default: all but missing).
    #[serde(default)]
    pub kinds: Vec<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TmTranslateInput {
    #[serde(default)]
    pub langs: Vec<String>,
    /// Replace existing translations (default false).
    #[serde(default)]
    pub overwrite: bool,
    /// Minimum fuzzy score 50..100; omit to disable fuzzy matches.
    pub fuzzy: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TmLookupInput {
    pub text: String,
    pub source_lang: String,
    pub target_lang: String,
    /// Minimum fuzzy score (default 75).
    pub min_score: Option<u8>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WorkflowStatusInput {
    /// Run git fetch and query merge/pull requests on the server (default false).
    pub online: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct LangsInput {
    #[serde(default)]
    pub langs: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportInput {
    /// Target file: .xlsx, .csv, .txt, .xlf/.xliff or .tmx.
    pub path: String,
    #[serde(default)]
    pub langs: Vec<String>,
    /// Only rows with these statuses in any exported language.
    #[serde(default)]
    pub status: Vec<String>,
    /// Screenshots of the exported rows: auto (default: embedded in Excel, otherwise linked) | none | link | embed.
    pub screenshots: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PictureInput {
    /// Key of a picture row (`data.format` = picture in list_rows/get_row).
    pub key: String,
    pub source: Option<usize>,
    /// Language of the translation to show (default: the original).
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoadTranslationFileInput {
    /// Key of a picture or binary row.
    pub key: String,
    pub source: Option<usize>,
    /// Target language id.
    pub lang: String,
    /// Absolute path of the translated file (e.g. the localized image).
    pub path: String,
    /// Keep the translation linked to the file, so a changed file is read again on scan/build (default true).
    pub link: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotIdInput {
    /// Screenshot id as returned by list_rows/get_row (`screenshots[].id`).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AttachScreenshotInput {
    /// Absolute path of a PNG or JPEG image.
    pub path: String,
    /// Rows the screenshot shows, optionally with the area of their text.
    #[serde(default)]
    pub keys: Vec<ScreenshotKeyInput>,
    /// Screenshot id (default: from the file name; an existing id gets the new image and keeps its links).
    pub id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DetachScreenshotInput {
    pub id: String,
    /// Rows to unlink; without keys the screenshot is removed.
    #[serde(default)]
    pub keys: Vec<ScreenshotKeyInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportInput {
    /// Exchange file: .xlsx, .csv, .txt, .xlf/.xliff or .tmx.
    pub path: String,
    #[serde(default)]
    pub langs: Vec<String>,
    /// Overwrite existing different translations (default true).
    pub overwrite: Option<bool>,
}

fn text(r: ToolResult) -> Result<String, String> {
    r.map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string()))
}

impl SusiServer {
    pub fn new(session: Session) -> Self {
        Self { state: Arc::new(Mutex::new(session)), tool_router: Self::tool_router() }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Session) -> T) -> Result<T, String> {
        let mut s = self.state.lock().map_err(|_| "session lock poisoned".to_string())?;
        Ok(f(&mut s))
    }

    fn run(&self, f: impl FnOnce(&mut Session) -> ToolResult) -> Result<String, String> {
        text(self.with(f)?)
    }
}

#[tool_router]
impl SusiServer {
    #[tool(description = "Open a Sisulizer/Susi project (.slp). Returns languages, sources and progress.")]
    fn open_project(&self, Parameters(i): Parameters<PathInput>) -> Result<String, String> {
        self.run(|s| s.open(&PathBuf::from(&i.path)))
    }

    #[tool(description = "Create a new empty project file with the given languages.")]
    fn create_project(&self, Parameters(i): Parameters<CreateInput>) -> Result<String, String> {
        self.run(|s| s.create(&PathBuf::from(&i.path), &i.languages))
    }

    #[tool(description = "Save the open project (optionally to a new path). Also saves pending TM changes.")]
    fn save_project(&self, Parameters(i): Parameters<OptPathInput>) -> Result<String, String> {
        self.run(|s| s.save(i.path.as_deref().map(std::path::Path::new)))
    }

    #[tool(description = "Languages, sources (index, file, type, output pattern) and translation progress of the open project.")]
    fn project_info(&self) -> Result<String, String> {
        self.run(|s| s.info())
    }

    #[tool(
        description = "List rows with original, translations (text/status/comment/date), row comment, max_chars, placeholders and created/changed dates. Supports filters (status, row status, text, source, date range via since/until/date_field) and paging. Use since/until to review recent changes."
    )]
    fn list_rows(&self, Parameters(i): Parameters<ListRowsInput>) -> Result<String, String> {
        self.run(|s| s.list_rows(&i))
    }

    #[tool(description = "Get a single row by key.")]
    fn get_row(&self, Parameters(i): Parameters<RowKeyInput>) -> Result<String, String> {
        self.run(|s| s.get_row(&i.key, i.source))
    }

    #[tool(description = "Write translations (batch). Default status is 'auto' (Auto translated); use 'translated' or 'complete' only for verified texts.")]
    fn set_translations(&self, Parameters(i): Parameters<SetTranslationsInput>) -> Result<String, String> {
        self.run(|s| s.set_translations(&i.items))
    }

    #[tool(description = "Change the translation status of rows for one language.")]
    fn set_status(&self, Parameters(i): Parameters<SetStatusInput>) -> Result<String, String> {
        self.run(|s| s.set_status(&i.keys, i.source, &i.lang, &i.status))
    }

    #[tool(
        description = "Copy the original or the translations of another language into one language, e.g. to fill de-CH from de, without transferring the texts. Only empty cells unless overwrite; skips do-not-translate and excluded rows and rows without text to copy. The status changes as when editing (untranslated, best guess and auto become translated). Returns copied and kept (translated cells left unchanged) counts."
    )]
    fn copy_translations(&self, Parameters(i): Parameters<CopyTranslationsInput>) -> Result<String, String> {
        self.run(|s| s.copy_translations(i.from.as_deref(), &i.lang, &i.keys, i.source, i.overwrite))
    }

    #[tool(description = "Set row properties: comment, do-not-translate flag, max characters.")]
    fn set_row_properties(&self, Parameters(i): Parameters<RowPropsInput>) -> Result<String, String> {
        self.run(|s| s.set_row_props(&i.key, i.source, i.comment.as_deref(), i.do_not_translate, i.max_chars))
    }

    #[tool(description = "Add a target language to the project.")]
    fn add_language(&self, Parameters(i): Parameters<LangInput>) -> Result<String, String> {
        self.run(|s| s.add_language(&i.lang))
    }

    #[tool(
        description = "Add a source file (.json, .resx, .resw, .po, .pot, .tsv, .properties, .arb, Qt .ts, Android res/values/*.xml or WebExtension _locales/<lang>/messages.json) to the project and scan it."
    )]
    fn add_source(&self, Parameters(i): Parameters<AddSourceInput>) -> Result<String, String> {
        self.run(|s| s.add_source(&PathBuf::from(&i.file), i.original_lang.as_deref().unwrap_or("en")))
    }

    #[tool(description = "Scan (re-read) the source files: new strings are added, changed originals invalidate translations, missing strings become unused.")]
    fn scan(&self, Parameters(i): Parameters<SourcesInput>) -> Result<String, String> {
        self.run(|s| s.scan(i.sources.as_deref()))
    }

    #[tool(description = "Build: write the localized files for the given languages (default all).")]
    fn build(&self, Parameters(i): Parameters<BuildInput>) -> Result<String, String> {
        self.run(|s| s.build(&i.langs, i.sources.as_deref()))
    }

    #[tool(description = "Validate translations (placeholders, tags, whitespace, line breaks, punctuation, length, hotkeys, duplicates).")]
    fn validate(&self, Parameters(i): Parameters<ValidateInput>) -> Result<String, String> {
        self.run(|s| s.validate(&i.langs, &i.kinds))
    }

    #[tool(description = "Translation statistics per language and status.")]
    fn statistics(&self) -> Result<String, String> {
        self.run(|s| s.statistics())
    }

    #[tool(description = "Translate untranslated rows from the translation memory (exact -> 'auto', fuzzy -> 'best').")]
    fn tm_translate(&self, Parameters(i): Parameters<TmTranslateInput>) -> Result<String, String> {
        self.run(|s| s.tm_translate(&i.langs, i.overwrite, i.fuzzy))
    }

    #[tool(description = "Look up a text in the translation memory (exact and fuzzy matches).")]
    fn tm_lookup(&self, Parameters(i): Parameters<TmLookupInput>) -> Result<String, String> {
        self.run(|s| s.tm_lookup(&i.text, &i.source_lang, &i.target_lang, i.min_score.unwrap_or(75)))
    }

    #[tool(description = "Store translated/complete translations of the project in the translation memory.")]
    fn tm_save(&self, Parameters(i): Parameters<LangsInput>) -> Result<String, String> {
        self.run(|s| s.tm_save(&i.langs))
    }

    #[tool(description = "Export rows for translators (.xlsx, .csv, .txt, .xlf, .tmx).")]
    fn export(&self, Parameters(i): Parameters<ExportInput>) -> Result<String, String> {
        self.run(|s| s.export(&PathBuf::from(&i.path), &i.langs, &i.status, i.screenshots.as_deref()))
    }

    #[tool(
        description = "Get a screenshot of rows as image, with the rows it shows (key, original, area = [x, y, width, height] in pixels). Use it to see where and in which context a text appears and how much space it has."
    )]
    fn get_screenshot(&self, Parameters(i): Parameters<ScreenshotIdInput>) -> Result<CallToolResult, String> {
        use base64::Engine as _;
        let (bytes, mime, info) = self.with(|s| s.screenshot(&i.id))??;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(CallToolResult::success(vec![ContentBlock::image(data, mime), ContentBlock::text(text(Ok(info))?)]))
    }

    #[tool(
        description = "Get the picture of a picture row (images in .resx files and Sisulizer projects) as image: the original, or the translation in lang. Most clients display PNG, JPEG and GIF."
    )]
    fn get_picture(&self, Parameters(i): Parameters<PictureInput>) -> Result<CallToolResult, String> {
        use base64::Engine as _;
        let (bytes, mime, info) = self.with(|s| s.picture(&i.key, i.source, i.lang.as_deref()))??;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(CallToolResult::success(vec![ContentBlock::image(data, mime), ContentBlock::text(text(Ok(info))?)]))
    }

    #[tool(
        description = "Set the translation of a picture or binary row from a file (e.g. a localized image), linked to the file by default so that a changed file is read again on scan/build. The status changes as when editing."
    )]
    fn load_translation_file(&self, Parameters(i): Parameters<LoadTranslationFileInput>) -> Result<String, String> {
        self.run(|s| s.load_translation_file(&i.key, i.source, &i.lang, &PathBuf::from(&i.path), i.link.unwrap_or(true)))
    }

    #[tool(
        description = "Attach a screenshot (PNG/JPEG file) to rows, optionally with the area of each text. Stored next to the project (<project>.files/) and saved with save_project."
    )]
    fn attach_screenshot(&self, Parameters(i): Parameters<AttachScreenshotInput>) -> Result<String, String> {
        self.run(|s| s.attach_screenshot(&PathBuf::from(&i.path), &i.keys, i.id.as_deref(), i.title.as_deref()))
    }

    #[tool(description = "Unlink a screenshot from rows (keys), or remove it entirely (no keys).")]
    fn detach_screenshot(&self, Parameters(i): Parameters<DetachScreenshotInput>) -> Result<String, String> {
        self.run(|s| s.detach_screenshot(&i.id, &i.keys))
    }

    #[tool(description = "Import translations from an exchange file (.xlsx, .csv, .txt, .xlf, .tmx).")]
    fn import(&self, Parameters(i): Parameters<ImportInput>) -> Result<String, String> {
        self.run(|s| s.import(&PathBuf::from(&i.path), &i.langs, i.overwrite.unwrap_or(true)))
    }

    #[tool(
        description = "Git workflow state of the open project: up_to_date | update_available | modified | saved | in_review | published | detached, with changed project files, branch, provider (gitlab, github) and merge/pull request."
    )]
    fn workflow_status(&self, Parameters(i): Parameters<WorkflowStatusInput>) -> Result<String, String> {
        self.run(|s| s.workflow_status(i.online.unwrap_or(false)))
    }

    #[tool(
        description = "Get translation project: sets local changes aside (git stash) and checks out the latest version of the default branch (or a given branch/commit), then reopens the project."
    )]
    fn workflow_fetch(&self, Parameters(i): Parameters<WorkflowFetchInput>) -> Result<String, String> {
        self.run(|s| s.workflow_fetch(&i))
    }

    #[tool(
        description = "Save changes: builds the localized files, saves the project and commits/pushes the changed project files to a personal branch (created from the configured pattern when on the default branch)."
    )]
    fn workflow_save(&self, Parameters(i): Parameters<WorkflowSaveInput>) -> Result<String, String> {
        self.run(|s| s.workflow_save(&i))
    }

    #[tool(description = "Prepare publication: creates or updates the merge request (GitLab) or pull request (GitHub) of the current branch with reviewers.")]
    fn workflow_publish(&self, Parameters(i): Parameters<WorkflowPublishInput>) -> Result<String, String> {
        self.run(|s| s.workflow_publish(&i))
    }

    #[tool(description = "Fill pseudo translations for a (pseudo) language to test the UI.")]
    fn fill_pseudo(&self, Parameters(i): Parameters<LangInput>) -> Result<String, String> {
        self.run(|s| s.fill_pseudo(&i.lang))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SusiServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("susi", env!("CARGO_PKG_VERSION")).with_title("Susi the CAT"))
            .with_instructions(INSTRUCTIONS)
    }
}
