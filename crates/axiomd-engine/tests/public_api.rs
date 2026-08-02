//! The engine boundary is sealed: no comrak type may appear in axiomd-engine's
//! public API.
//!
//! Two independent guards, because neither alone is complete:
//!
//! 1. [`the_whole_public_api_is_usable_without_comrak`] is a compile-time proof.
//!    Integration tests link against the crate's *public* interface only — comrak is
//!    a private dependency and is not in scope here — so if any public item required
//!    a comrak type to name, construct or consume, this file would not compile.
//! 2. [`comrak_is_confined_to_one_module`] inspects the crate's own source. This is
//!    a declaration-level invariant ("no public signature mentions comrak"), so
//!    reading the declarations is a direct check of it rather than a proxy for
//!    behaviour. It catches the leak the compile-time guard cannot: a public item
//!    whose comrak type is inferrable and therefore never named at the call site.

use std::path::Path;

use axiomd_engine::{
    Alignment, Callout, CalloutKind, ComrakEngine, EngineId, Event, Extension, Extensions,
    MarkdownEngine, Parsed, Span, SpannedEvent, Tag, TagEnd,
};

#[test]
fn the_whole_public_api_is_usable_without_comrak() {
    // Options.
    let extensions = Extensions::COMMONMARK | Extensions::GFM | Extension::Math;
    assert!(extensions.contains(Extension::Math));
    assert!(
        extensions
            .intersection(Extensions::GFM)
            .contains(Extension::Tables)
    );
    assert!(extensions.iter().count() >= 5);
    assert_eq!(Extension::ALL.len(), 9);

    // Identity.
    let engine = ComrakEngine::new();
    let id: EngineId = engine.id();
    assert_eq!(id, ComrakEngine::ID);
    assert_eq!(EngineId::new("comrak").as_str(), id.as_str());

    // Parsing, through the trait and through the concrete type.
    let engine: &dyn MarkdownEngine = &engine;
    assert_eq!(engine.capabilities(), Extensions::FULL);
    let source = "> [!TIP] Do it\n>\n> | a |\n> | :- |\n> | `x` |\n\n![alt](img.png \"t\")\n";
    let parsed: Parsed<'_> = engine.parse(source, Extensions::FULL);
    assert_eq!(parsed.front_matter(), None);

    // Events, spans, and every payload-carrying variant, named explicitly.
    let events: &[SpannedEvent<'_>] = parsed.events();
    assert!(!events.is_empty());
    for SpannedEvent { event, span } in events {
        let Span { range, line } = span;
        assert!(range.end <= source.len() && *line >= 1);
        match event {
            Event::Start(tag) => match tag {
                Tag::BlockQuote {
                    callout: Some(Callout { kind, title }),
                } => {
                    assert_eq!(*kind, CalloutKind::Tip);
                    assert_eq!(title.as_deref(), Some("Do it"));
                }
                Tag::TableCell { alignment } => assert_eq!(*alignment, Alignment::Left),
                Tag::Image { url, title } => {
                    assert_eq!(url.as_ref(), "img.png");
                    assert_eq!(title.as_ref(), "t");
                }
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote { .. }
                | Tag::CodeBlock { .. }
                | Tag::List { .. }
                | Tag::Item { .. }
                | Tag::FootnoteDefinition { .. }
                | Tag::Table { .. }
                | Tag::TableHead
                | Tag::TableRow
                | Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::WikiLink { .. } => {}
            },
            Event::End(end) => {
                assert!(!matches!(end, TagEnd::Heading(0)));
            }
            Event::Text(_)
            | Event::Code(_)
            | Event::Math { .. }
            | Event::HtmlBlock(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::ThematicBreak => {}
        }
    }

    // Engines outside this crate can produce results too.
    let handmade = Parsed::new(
        vec![SpannedEvent {
            event: Event::ThematicBreak,
            span: Span {
                range: 0..3,
                line: 1,
            },
        }],
        Some("---\n"),
    );
    assert_eq!(handmade.events().len(), 1);
    assert_eq!(handmade.front_matter(), Some("---\n"));
}

/// The one module allowed to import from comrak.
const COMRAK_HOME: &str = "comrak_engine.rs";

#[test]
fn comrak_is_confined_to_one_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = walk(&src);
    assert!(
        files.len() >= 3,
        "only {} source files found; the walk is broken",
        files.len()
    );

    let mut imported: Vec<String> = Vec::new();
    for path in &files {
        let name = file_name(path);
        let code = strip_comments(&std::fs::read_to_string(path).expect("reading crate source"));
        let imports = comrak_imports(&code);

        assert!(
            imports.is_empty() || name == COMRAK_HOME,
            "{name} imports from comrak; only {COMRAK_HOME} may: {imports:?}"
        );
        assert!(
            !code.contains("comrak::") || name == COMRAK_HOME,
            "{name} names a comrak path; only {COMRAK_HOME} may"
        );
        imported.extend(imports);
    }

    assert!(
        !imported.is_empty(),
        "no comrak imports found at all; the import scan is broken"
    );

    let home = files
        .iter()
        .find(|p| file_name(p) == COMRAK_HOME)
        .unwrap_or_else(|| panic!("{COMRAK_HOME} not found"));
    let code = strip_comments(&std::fs::read_to_string(home).expect("reading crate source"));
    let mut declarations = 0usize;
    for (number, declaration) in public_declarations(&code) {
        declarations += 1;
        for name in &imported {
            assert!(
                !mentions(&declaration, name),
                "{COMRAK_HOME}:{number} leaks the comrak type {name} through a public item: {declaration}"
            );
        }
        assert!(
            !declaration.contains("comrak::"),
            "{COMRAK_HOME}:{number} leaks a comrak path through a public item: {declaration}"
        );
    }
    assert!(
        declarations >= 3,
        "only {declarations} public declarations found in {COMRAK_HOME}; the scan is broken"
    );
}

/// Every public declaration in a file, each joined into a single line so that a
/// rustfmt-wrapped signature is checked whole.
fn public_declarations(code: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = code.lines().map(str::trim).collect();
    let mut declarations = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if declares_public_item(lines[index]) {
            let start = index;
            let mut joined = String::new();
            while index < lines.len() {
                joined.push_str(lines[index]);
                joined.push(' ');
                if lines[index].contains('{') || lines[index].ends_with(';') {
                    break;
                }
                index += 1;
            }
            declarations.push((start + 1, joined.trim_end().to_string()));
        }
        index += 1;
    }
    declarations
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .expect("source file name")
        .to_string()
}

/// The identifiers a file brings into scope from comrak, including `as` aliases.
fn comrak_imports(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = code;
    while let Some(at) = find_comrak_use(rest) {
        let statement = &rest[at..];
        let end = statement.find(';').expect("unterminated use statement");
        let statement = &statement[..end];
        for part in statement
            .trim_start_matches("use ")
            .split(['{', '}', ',', ' ', '\n'])
            .filter(|p| !p.is_empty())
        {
            // `a::b::C` imports `C`; `X as Y` imports `Y`, and `as`/`X` are handled
            // by taking the last identifier of the statement's comma-separated parts.
            let last = part.rsplit("::").next().unwrap_or(part);
            if last != "as" && last.chars().next().is_some_and(|c| c.is_alphabetic()) {
                names.push(last.to_string());
            }
        }
        // An alias shadows the name it renames.
        rest = &rest[at + end..];
    }
    names.retain(|n| n != "comrak");
    names
}

/// The next `use comrak…` statement that imports from the comrak crate itself,
/// rather than from a module whose name merely starts with `comrak`.
fn find_comrak_use(code: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = code[from..].find("use comrak") {
        let at = from + at;
        let after = code[at + "use comrak".len()..].chars().next();
        if !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return Some(at);
        }
        from = at + "use comrak".len();
    }
    None
}

/// Whether `line` uses `name` as a standalone identifier.
fn mentions(line: &str, name: &str) -> bool {
    line.match_indices(name).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        boundary(before) && boundary(after)
    })
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("reading crate source directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files
}

/// Blanks out `//` comments line by line, preserving line numbering, so that prose
/// may discuss comrak freely while declarations may not mention it.
fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a line opens a public declaration — a public item, or a public field or
/// enum variant payload inside one. `pub(crate)` and friends are not public.
fn declares_public_item(line: &str) -> bool {
    line.starts_with("pub ") && !line.starts_with("pub(")
}
