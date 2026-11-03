//! rmcp tool router exposing a [`Session`] over MCP.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::session::{ListRowsInput, Session, ToolResult, TranslationInput};

const INSTRUCTIONS: &str = "Susi the CAT (Sisulizer compatible). Typical AI translation workflow: \
open_project(path) -> project_info -> list_rows(status=[\"untranslated\"], langs=[\"de\"]) -> \
set_translations([{key, lang, text}], status defaults to 'auto') -> validate -> save_project. \
Keep placeholders ({0}, {name}, %s, <tags>), leading/trailing spaces, line breaks and '&' hotkeys exactly as in the original; \
respect max_chars and row comments. Rows marked do_not_translate must not be translated. \
scan updates the project from the source files, build writes the localized files.";

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
    /// Path of a .json, .resx, .po, .pot or .tsv file.
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
    /// Checks: missing, format, tags, whitespace, newline, period, colon, exclamation, question, length, allspaces, hotkey, duplicate (default: all but missing).
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

    #[tool(description = "List rows with original, translations (text/status/comment), row comment, max_chars and placeholders. Supports filters and paging.")]
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

    #[tool(description = "Set row properties: comment, do-not-translate flag, max characters.")]
    fn set_row_properties(&self, Parameters(i): Parameters<RowPropsInput>) -> Result<String, String> {
        self.run(|s| s.set_row_props(&i.key, i.source, i.comment.as_deref(), i.do_not_translate, i.max_chars))
    }

    #[tool(description = "Add a target language to the project.")]
    fn add_language(&self, Parameters(i): Parameters<LangInput>) -> Result<String, String> {
        self.run(|s| s.add_language(&i.lang))
    }

    #[tool(description = "Add a source file (.json, .resx, .po, .pot or .tsv) to the project and scan it.")]
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
        self.run(|s| s.export(&PathBuf::from(&i.path), &i.langs, &i.status))
    }

    #[tool(description = "Import translations from an exchange file (.xlsx, .csv, .txt, .xlf, .tmx).")]
    fn import(&self, Parameters(i): Parameters<ImportInput>) -> Result<String, String> {
        self.run(|s| s.import(&PathBuf::from(&i.path), &i.langs, i.overwrite.unwrap_or(true)))
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
