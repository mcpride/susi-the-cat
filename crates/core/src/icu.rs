//! ICU MessageFormat (Flutter ARB, FormatJS/react-intl, i18next-icu, Java `MessageFormat`).
//!
//! A light parser for validation: it checks the syntax and returns the arguments with their
//! types and the cases of `plural`, `selectordinal` and `select` arguments. Message text is not
//! kept. Apostrophes follow ICU's default (`DOUBLE_OPTIONAL`): `''` is an apostrophe, `'` starts
//! quoted text only before `{`, `}`, `|` and, in plural cases, `#`; any other `'` is literal.

use std::collections::BTreeMap;
use std::fmt;

/// Plural categories (CLDR); explicit values are written `=0`, `=1`, ….
pub const PLURAL_CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// Argument types besides `plural`, `selectordinal` and `select` (`choice`: Java).
const SIMPLE_TYPES: [&str; 7] = ["number", "date", "time", "spellout", "ordinal", "duration", "choice"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// `{name}`
    Simple,
    /// `{name, number}`, `{d, date, short}`: the type in lower case.
    Typed(String),
    /// `{n, plural, one{…} other{…}}`; `ordinal` for `selectordinal`.
    Plural { ordinal: bool, cases: Vec<Case> },
    /// `{g, select, male{…} other{…}}`
    Select { cases: Vec<Case> },
}

/// A case of a plural or select argument: its selector (`one`, `=0`, `male`) and the arguments of
/// its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub selector: String,
    pub args: Vec<Arg>,
}

/// A syntax error; positions are 1-based character positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A `{` without its `}`.
    Unclosed(usize),
    /// A `}` without its `{`.
    Unmatched(usize),
    /// No argument name after `{`.
    Name(usize),
    /// Neither `,` nor `}` after a name or type.
    Expected(usize),
    /// No `{` after a case selector, or no selector.
    Case(usize),
    /// `offset:` without a number.
    Offset(usize),
    UnknownType {
        arg: String,
        ty: String,
    },
    /// A plural case that is neither a category nor `=N` (often a translated keyword).
    Category {
        arg: String,
        selector: String,
    },
    MissingOther(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Error::Unclosed(pos) => crate::tf!("'{' at position {pos} is not closed", pos = pos),
            Error::Unmatched(pos) => crate::tf!("'}' at position {pos} has no matching '{'", pos = pos),
            Error::Name(pos) => crate::tf!("argument name expected at position {pos}", pos = pos),
            Error::Expected(pos) => crate::tf!("',' or '}' expected at position {pos}", pos = pos),
            Error::Case(pos) => crate::tf!("case and '{' expected at position {pos}", pos = pos),
            Error::Offset(pos) => crate::tf!("number expected after 'offset:' at position {pos}", pos = pos),
            Error::UnknownType { arg, ty } => crate::tf!("{{arg}}: unknown type '{kind}'", arg = arg, kind = ty),
            Error::Category { arg, selector } => {
                crate::tf!("{{arg}}: '{case}' is no plural category (zero, one, two, few, many, other, =N)", arg = arg, case = selector)
            }
            Error::MissingOther(arg) => crate::tf!("{{arg}}: the case 'other' is missing", arg = arg),
        };
        f.write_str(&s)
    }
}

/// Parses a message and returns its top-level arguments.
pub fn parse(s: &str) -> Result<Vec<Arg>, Error> {
    let chars: Vec<char> = s.chars().collect();
    let mut p = Parser { c: &chars, i: 0 };
    let args = p.message(0, false)?;
    // `message` returns at a `}` only when nested; at the top level it is unmatched.
    match p.i < chars.len() {
        true => Err(Error::Unmatched(p.i + 1)),
        false => Ok(args),
    }
}

/// Whether the arguments use ICU syntax beyond `{name}` (typed, plural or select arguments).
pub fn is_complex(args: &[Arg]) -> bool {
    args.iter().any(|a| a.kind != Kind::Simple)
}

/// Whether `s` contains a plural or select argument, even if it does not parse.
pub fn looks_like_icu(s: &str) -> bool {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    [",plural,", ",select,", ",selectordinal,"].iter().any(|k| compact.contains(k))
}

/// Argument names with their type (`plural`, `number`, … or empty for `{name}` only), including
/// those of nested cases. A `{n}` next to `{n, plural, …}` counts as the plural argument.
pub fn signature(args: &[Arg]) -> BTreeMap<String, String> {
    fn walk(args: &[Arg], out: &mut BTreeMap<String, String>) {
        for a in args {
            let ty = match &a.kind {
                Kind::Simple => "",
                Kind::Typed(t) => t,
                Kind::Plural { ordinal: false, .. } => "plural",
                Kind::Plural { ordinal: true, .. } => "selectordinal",
                Kind::Select { .. } => "select",
            };
            let slot = out.entry(a.name.clone()).or_default();
            if slot.is_empty() {
                *slot = ty.to_string();
            }
            if let Kind::Plural { cases, .. } | Kind::Select { cases } = &a.kind {
                for c in cases {
                    walk(&c.args, out);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(args, &mut out);
    out
}

/// The signature as a list for messages: `{count, plural}`, `{name}`.
pub fn signature_list(args: &[Arg]) -> Vec<String> {
    signature(args).into_iter().map(|(n, t)| if t.is_empty() { format!("{{{n}}}") } else { format!("{{{n}, {t}}}") }).collect()
}

/// Cases missing in a translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// Plural categories the target language uses, missing in a cardinal plural argument.
    Plural { arg: String, categories: Vec<String> },
    /// Select cases of the original, missing in the translation's select argument of that name.
    Select { arg: String, cases: Vec<String> },
}

/// Compares the cases of a translation with the original. `categories` are the plural categories
/// of the target language (CLDR, integers), `None` when unknown. An explicit `=0`, `=1` or `=2`
/// stands in for `zero`, `one` or `two`, as in `=1{One file} other{# files}`.
pub fn missing_cases(original: &[Arg], translation: &[Arg], categories: Option<&[&str]>) -> Vec<Missing> {
    let mut out = Vec::new();
    let mut plurals = Vec::new();
    let mut selects = Vec::new();
    collect(translation, &mut plurals, &mut selects);
    if let Some(needed) = categories {
        for (arg, cases) in &plurals {
            let has = |c: &str| {
                let explicit = match c {
                    "zero" => "=0",
                    "one" => "=1",
                    "two" => "=2",
                    _ => "",
                };
                cases.contains(&c) || (!explicit.is_empty() && cases.contains(&explicit))
            };
            let missing: Vec<String> = needed.iter().filter(|c| !has(c)).map(|c| c.to_string()).collect();
            if !missing.is_empty() {
                out.push(Missing::Plural { arg: arg.to_string(), categories: missing });
            }
        }
    }
    let (mut orig_plurals, mut orig_selects) = (Vec::new(), Vec::new());
    collect(original, &mut orig_plurals, &mut orig_selects);
    for (arg, orig_cases) in &orig_selects {
        let Some((_, cases)) = selects.iter().find(|(a, _)| a == arg) else { continue };
        let missing: Vec<String> = orig_cases.iter().filter(|c| !cases.contains(*c)).map(|c| c.to_string()).collect();
        if !missing.is_empty() && !out.iter().any(|m| matches!(m, Missing::Select { arg: a, .. } if a == arg)) {
            out.push(Missing::Select { arg: arg.to_string(), cases: missing });
        }
    }
    out
}

/// Cardinal plural and select arguments with their selectors.
type Cases<'a> = Vec<(&'a str, Vec<&'a str>)>;

fn collect<'a>(args: &'a [Arg], plurals: &mut Cases<'a>, selects: &mut Cases<'a>) {
    for a in args {
        if let Kind::Plural { cases, .. } | Kind::Select { cases } = &a.kind {
            let selectors = cases.iter().map(|c| c.selector.as_str()).collect();
            match &a.kind {
                Kind::Plural { ordinal: false, .. } => plurals.push((&a.name, selectors)),
                Kind::Select { .. } => selects.push((&a.name, selectors)),
                _ => {}
            }
            for c in cases {
                collect(&c.args, plurals, selects);
            }
        }
    }
}

struct Parser<'a> {
    c: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn ident(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            self.i += 1;
        }
        self.c[start..self.i].iter().collect()
    }

    /// Message text with arguments, up to a `}` that closes it (not consumed) or the end.
    fn message(&mut self, depth: usize, plural: bool) -> Result<Vec<Arg>, Error> {
        let mut args = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                '\'' => self.quote(plural),
                '{' => args.push(self.arg(depth)?),
                '}' => return if depth > 0 { Ok(args) } else { Err(Error::Unmatched(self.i + 1)) },
                _ => self.i += 1,
            }
        }
        Ok(args)
    }

    /// Skips an apostrophe and the text it quotes.
    fn quote(&mut self, plural: bool) {
        let next = self.c.get(self.i + 1).copied();
        match next {
            Some('\'') => self.i += 2,
            Some('{' | '}' | '|') => self.quoted(),
            Some('#') if plural => self.quoted(),
            _ => self.i += 1,
        }
    }

    /// Quoted text up to the closing apostrophe (`''` inside is an apostrophe); unterminated
    /// quotes run to the end, as in ICU.
    fn quoted(&mut self) {
        self.i += 1;
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == '\'' {
                if self.peek() == Some('\'') {
                    self.i += 1;
                } else {
                    return;
                }
            }
        }
    }

    fn arg(&mut self, depth: usize) -> Result<Arg, Error> {
        let open = self.i + 1;
        self.i += 1;
        self.ws();
        let name = self.ident();
        if name.is_empty() {
            return Err(if self.peek().is_none() { Error::Unclosed(open) } else { Error::Name(self.i + 1) });
        }
        self.ws();
        if !self.comma_or_close(open)? {
            return Ok(Arg { name, kind: Kind::Simple });
        }
        self.ws();
        let ty = self.ident();
        if ty.is_empty() {
            return Err(if self.peek().is_none() { Error::Unclosed(open) } else { Error::Expected(self.i + 1) });
        }
        let ty = ty.to_ascii_lowercase();
        let complex = matches!(ty.as_str(), "plural" | "selectordinal" | "select");
        if !complex && !SIMPLE_TYPES.contains(&ty.as_str()) {
            return Err(Error::UnknownType { arg: name, ty });
        }
        self.ws();
        if !self.comma_or_close(open)? {
            return if complex { Err(Error::Case(self.i)) } else { Ok(Arg { name, kind: Kind::Typed(ty) }) };
        }
        if !complex {
            self.style(open)?;
            return Ok(Arg { name, kind: Kind::Typed(ty) });
        }
        let cases = self.cases(open, &name, ty != "select", depth)?;
        let kind = match ty.as_str() {
            "select" => Kind::Select { cases },
            t => Kind::Plural { ordinal: t == "selectordinal", cases },
        };
        Ok(Arg { name, kind })
    }

    /// Consumes `,` (returns `true`) or `}` (returns `false`).
    fn comma_or_close(&mut self, open: usize) -> Result<bool, Error> {
        match self.peek() {
            Some(',') => {
                self.i += 1;
                Ok(true)
            }
            Some('}') => {
                self.i += 1;
                Ok(false)
            }
            None => Err(Error::Unclosed(open)),
            Some(_) => Err(Error::Expected(self.i + 1)),
        }
    }

    /// Skips the style of a simple argument (`#,##0.00`, `short`, Java choice cases) up to the
    /// closing `}`.
    fn style(&mut self, open: usize) -> Result<(), Error> {
        let mut depth = 0;
        while let Some(c) = self.peek() {
            match c {
                '\'' => self.quote(false),
                '{' => {
                    depth += 1;
                    self.i += 1;
                }
                '}' if depth == 0 => {
                    self.i += 1;
                    return Ok(());
                }
                '}' => {
                    depth -= 1;
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        Err(Error::Unclosed(open))
    }

    fn cases(&mut self, open: usize, name: &str, plural: bool, depth: usize) -> Result<Vec<Case>, Error> {
        let mut cases = Vec::new();
        self.ws();
        if plural && self.c[self.i..].starts_with(&['o', 'f', 'f', 's', 'e', 't', ':']) {
            self.i += 7;
            self.ws();
            let start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == start {
                return Err(Error::Offset(self.i + 1));
            }
        }
        loop {
            self.ws();
            match self.peek() {
                None => return Err(Error::Unclosed(open)),
                Some('}') => {
                    self.i += 1;
                    break;
                }
                Some(_) => {}
            }
            let at = self.i + 1;
            let selector = if plural && self.peek() == Some('=') {
                self.i += 1;
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                if self.i == start {
                    return Err(Error::Case(at));
                }
                format!("={}", self.c[start..self.i].iter().collect::<String>())
            } else {
                self.ident()
            };
            if selector.is_empty() {
                return Err(Error::Case(at));
            }
            if plural && !selector.starts_with('=') && !PLURAL_CATEGORIES.contains(&selector.as_str()) {
                return Err(Error::Category { arg: name.to_string(), selector });
            }
            self.ws();
            if self.peek() != Some('{') {
                return Err(if self.peek().is_none() { Error::Unclosed(open) } else { Error::Case(self.i + 1) });
            }
            let brace = self.i + 1;
            self.i += 1;
            let args = self.message(depth + 1, plural)?;
            if self.peek() != Some('}') {
                return Err(Error::Unclosed(brace));
            }
            self.i += 1;
            cases.push(Case { selector, args });
        }
        if !cases.iter().any(|c| c.selector == "other") {
            return Err(Error::MissingOther(name.to_string()));
        }
        Ok(cases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(s: &str) -> Vec<String> {
        signature_list(&parse(s).unwrap())
    }

    #[test]
    fn parses_arguments_types_and_nested_cases() {
        assert_eq!(sig("Hello {name}!"), ["{name}"]);
        assert_eq!(sig("{count, plural, =0{No wombats} one{# wombat} other{{count} wombats}}"), ["{count, plural}"]);
        assert_eq!(
            sig("{g, select, male{He has {n, plural, one{# file} other{# files}}} other{They have {n, plural, other{# files}}}} on {d, date, short}"),
            ["{d, date}", "{g, select}", "{n, plural}"]
        );
        assert_eq!(
            sig("{price, number, ::currency/EUR} {pos, selectordinal, one{#st} two{#nd} few{#rd} other{#th}}"),
            ["{pos, selectordinal}", "{price, number}"]
        );
        assert_eq!(sig("{n, plural, offset:1 =0{nobody} =1{{host}} other{{host} and # others}}"), ["{host}", "{n, plural}"]);
        // Java choice styles are skipped as a whole.
        assert_eq!(sig("{0,choice,0#no files|1#one file|1<{0,number,integer} files}"), ["{0, choice}"]);
        assert!(is_complex(&parse("{n, number}").unwrap()) && !is_complex(&parse("{a} {b}").unwrap()));
    }

    #[test]
    fn apostrophes_follow_icu() {
        // `'` quotes only before syntax characters; `''` is an apostrophe.
        assert_eq!(sig("Don't use '{braces}', it''s {n, plural, one{'#' #} other{#}}"), ["{n, plural}"]);
        assert_eq!(sig("I'm {name}"), ["{name}"]);
    }

    #[test]
    fn reports_syntax_errors() {
        let e = |s: &str| parse(s).unwrap_err();
        assert_eq!(e("Hello {name"), Error::Unclosed(7));
        assert_eq!(e("Hello name}"), Error::Unmatched(11));
        assert_eq!(e("{ , plural}"), Error::Name(3));
        assert_eq!(e("{n plural}"), Error::Expected(4));
        assert_eq!(e("{n, plural, one{# Datei}}"), Error::MissingOther("n".into()));
        assert_eq!(e("{n, plural, eins{# Datei} other{# Dateien}}"), Error::Category { arg: "n".into(), selector: "eins".into() });
        assert_eq!(e("{n, Anzahl}"), Error::UnknownType { arg: "n".into(), ty: "anzahl".into() });
        assert_eq!(e("{n, plural, one # Datei other{x}}"), Error::Case(17));
        assert_eq!(e("{n, plural, one{# Datei} other{# Dateien}"), Error::Unclosed(1));
        assert_eq!(e("{n, plural, offset:x other{#}}"), Error::Offset(20));
        assert_eq!(e("{n, select, male{er}}"), Error::MissingOther("n".into()));
        // Select cases are free identifiers, plural `=N` only in plurals.
        assert!(parse("{g, select, female{sie} other{es}}").is_ok());
        assert!(parse("{g, select, =1{x} other{y}}").is_err());
    }

    #[test]
    fn missing_cases_for_the_target_language() {
        let orig = parse("{g, select, male{He} female{She} other{They}} has {n, plural, one{# file} other{# files}}").unwrap();
        let pl = parse("{g, select, male{On} other{Oni}} ma {n, plural, one{# plik} other{# plików}}").unwrap();
        let missing = missing_cases(&orig, &pl, Some(&["one", "few", "many", "other"]));
        assert_eq!(
            missing,
            [
                Missing::Plural { arg: "n".into(), categories: vec!["few".into(), "many".into()] },
                Missing::Select { arg: "g".into(), cases: vec!["female".into()] }
            ]
        );
        // `=1` stands in for `one`; unknown languages are not checked for plural categories.
        let de = parse("{n, plural, =0{Keine} =1{Eine Datei} other{# Dateien}}").unwrap();
        assert!(missing_cases(&de, &de, Some(&["one", "other"])).is_empty());
        assert!(missing_cases(&orig, &parse("{g, select, male{a} female{b} other{c}} {n, plural, other{#}}").unwrap(), None).is_empty());
    }

    #[test]
    fn detects_icu_that_does_not_parse() {
        assert!(looks_like_icu("{count, plural, one{# item} other{# items}"));
        assert!(looks_like_icu("{ gender , Select , male{x}}"));
        assert!(!looks_like_icu("Hello {name}, see {0:N2}"));
    }
}
