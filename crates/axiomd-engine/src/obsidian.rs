//! The two Obsidian constructs no engine parses for us, recognised on the finished
//! event stream.
//!
//! Both are shapes a CommonMark parser is *right* to leave alone — a callout is a
//! block quote whose first line happens to start with `[!kind]`, and an embed is a
//! wikilink a parser refuses because of the `!` in front of it — so recognising them
//! is axiomd's job rather than the engine's. Doing it here rather than inside one
//! parser's AST means every engine behind the boundary gets the same vocabulary, and
//! that the rules live in one readable place instead of in a traversal.
//!
//! # Why not comrak's own alerts
//!
//! comrak implements GitHub's five kinds and nothing else, and it folds Obsidian's
//! fold marker into the title (`[!note]- Folded` arrives with the title `"- Folded"`,
//! probed on comrak 0.54). Recognising the other fifteen kinds would then mean a
//! second, differently-shaped code path for the same construct. So the extension is
//! left off and every callout — GitHub's five included — is recognised here, by one
//! rule, with the fold marker read where it was written.
//!
//! # What this may not do
//!
//! Move anything. Source spans are load-bearing (invariant 3), so both passes only
//! ever *remove* events that were the marker itself, or split one text event into
//! pieces whose spans are slices of the span it had. Nothing is reordered and no
//! span is widened.

use std::borrow::Cow;

use crate::boundary::{Callout, Event, Span, SpannedEvent, Tag, TagEnd};

/// Recognises `> [!kind]` callouts, in place.
///
/// A block quote is a callout when its first paragraph opens with the marker. The
/// marker's own text — and the line break after it — stop being content, because they
/// are the callout's kind and title rather than its first sentence; everything else
/// the quote holds is untouched, which is what makes a callout nest inside a callout
/// without any special case.
pub(crate) fn recognise_callouts(events: &mut Vec<SpannedEvent<'_>>) {
    let mut at = 0;
    while at < events.len() {
        let Some(marker) = marker_at(events, at) else {
            at += 1;
            continue;
        };
        let Some(SpannedEvent {
            event: Event::Start(Tag::BlockQuote { callout }),
            ..
        }) = events.get_mut(at)
        else {
            at += 1;
            continue;
        };
        *callout = Some(marker.callout);
        // The marker text, and the soft break that ended its line: both are the
        // marker rather than the quote's prose.
        events.drain(at + 2..at + 2 + marker.consumed);
        // A callout whose first line was only the marker leaves an empty paragraph
        // behind, which would render as a blank line above the body.
        if matches!(
            events.get(at + 2),
            Some(SpannedEvent {
                event: Event::End(TagEnd::Paragraph),
                ..
            })
        ) {
            events.drain(at + 1..at + 3);
        }
        at += 1;
    }
}

/// What a marker turned out to be, and how many events it took up.
struct Marker {
    callout: Callout<'static>,
    /// Events after the opening paragraph that were the marker: the text itself, and
    /// the soft break ending its line when there was one.
    consumed: usize,
}

/// Reads the callout marker opening the block quote at `at`, if there is one.
fn marker_at(events: &[SpannedEvent<'_>], at: usize) -> Option<Marker> {
    match events.get(at)?.event {
        Event::Start(Tag::BlockQuote { callout: None }) => {}
        _ => return None,
    }
    match events.get(at + 1)?.event {
        Event::Start(Tag::Paragraph) => {}
        _ => return None,
    }
    let Event::Text(text) = &events.get(at + 2)?.event else {
        return None;
    };
    let (callout, rest) = parse_marker(text)?;
    // The title runs to the end of the marker's line, so the break that ends that line
    // belongs to the marker too — without this the callout's body would open with a
    // blank line where the marker used to be.
    let ends_the_line = matches!(
        events.get(at + 3),
        Some(SpannedEvent {
            event: Event::SoftBreak,
            ..
        })
    );
    let title = rest.trim();
    Some(Marker {
        callout: Callout {
            title: (!title.is_empty()).then(|| Cow::Owned(title.to_owned())),
            ..callout
        },
        consumed: if ends_the_line { 2 } else { 1 },
    })
}

/// Reads `[!kind]`, an optional fold marker and the title off the front of `text`,
/// answering with the callout and whatever is left of the text event.
///
/// The kind is anything between the brackets that is not a bracket, because
/// Obsidian's vocabulary is open: a kind nothing recognises is still a callout, and
/// what it should look like is the renderer's decision.
fn parse_marker(text: &str) -> Option<(Callout<'static>, &str)> {
    let rest = text.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let kind = &rest[..close];
    if kind.is_empty() || kind.contains('[') {
        return None;
    }
    let after = &rest[close + 1..];
    let (fold, after) = match after.as_bytes().first() {
        Some(b'+') => (Some(true), &after[1..]),
        Some(b'-') => (Some(false), &after[1..]),
        _ => (None, after),
    };
    // `[!note]x` is not a marker: a callout's kind is followed by the end of the
    // line or by whitespace before its title, and nothing else.
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some((
        Callout {
            kind: Cow::Owned(kind.to_lowercase()),
            title: None,
            fold,
        },
        after,
    ))
}

/// Recognises `![[target]]` embeds inside text, in place.
///
/// A CommonMark parser sees `!` followed by something that is not a link and leaves
/// the whole run as literal text, so the reader would be shown the brackets. This
/// turns each one into a wikilink marked as an embed — which the renderer shows as a
/// reference to something that is not here, transclusion being out of scope.
///
/// A text event is only split when the source it came from spells it exactly. Text
/// arrives with entity references and backslash escapes already resolved, and an
/// offset into resolved text is not an offset into the source; refusing those keeps
/// every span produced here a true slice of the one it came from.
pub(crate) fn recognise_embeds(events: &mut Vec<SpannedEvent<'_>>, source: &str) {
    let mut at = 0;
    while at < events.len() {
        let Some(split) = split_embeds(&events[at], source) else {
            at += 1;
            continue;
        };
        let taken = split.len();
        events.splice(at..at + 1, split);
        at += taken;
    }
}

/// One text event as the events it holds embeds for, or `None` when it holds none.
fn split_embeds<'a>(event: &SpannedEvent<'a>, source: &str) -> Option<Vec<SpannedEvent<'static>>> {
    let Event::Text(text) = &event.event else {
        return None;
    };
    if !text.contains("![[") {
        return None;
    }
    let span = &event.span;
    if source.get(span.range.clone()) != Some(text.as_ref()) {
        return None;
    }

    let mut split: Vec<SpannedEvent<'static>> = Vec::new();
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find("![[") {
        let open = cursor + open;
        let Some(close) = text[open..].find("]]").map(|end| open + end) else {
            break;
        };
        let target = &text[open + 3..close];
        if target.contains('[') || target.contains(']') || target.trim().is_empty() {
            cursor = open + 3;
            continue;
        }
        if open > cursor {
            split.push(sliced(
                event,
                source,
                cursor..open,
                Event::Text(Cow::Owned(text[cursor..open].to_owned())),
            ));
        }
        let (target, label) = match target.split_once('|') {
            Some((target, label)) => (target.trim(), label.trim()),
            None => (target.trim(), target.trim()),
        };
        let whole = open..close + 2;
        split.push(sliced(
            event,
            source,
            whole.clone(),
            Event::Start(Tag::WikiLink {
                target: Cow::Owned(target.to_owned()),
                embed: true,
            }),
        ));
        split.push(sliced(
            event,
            source,
            whole.clone(),
            Event::Text(Cow::Owned(label.to_owned())),
        ));
        split.push(sliced(event, source, whole, Event::End(TagEnd::WikiLink)));
        cursor = close + 2;
    }
    if split.is_empty() {
        return None;
    }
    if cursor < text.len() {
        split.push(sliced(
            event,
            source,
            cursor..text.len(),
            Event::Text(Cow::Owned(text[cursor..].to_owned())),
        ));
    }
    Some(split)
}

/// `event` cut down to the part of its own span that `within` names.
fn sliced(
    event: &SpannedEvent<'_>,
    source: &str,
    within: std::ops::Range<usize>,
    what: Event<'static>,
) -> SpannedEvent<'static> {
    let start = event.span.range.start + within.start;
    let end = event.span.range.start + within.end;
    SpannedEvent {
        event: what,
        span: Span {
            range: start..end,
            line: event.span.line
                + source[event.span.range.start..start].matches('\n').count() as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(text: &str) -> Option<(String, Option<bool>, String)> {
        parse_marker(text).map(|(callout, rest)| {
            (
                callout.kind.into_owned(),
                callout.fold,
                rest.trim().to_owned(),
            )
        })
    }

    /// The grammar, kind by kind: what is a marker and what is prose that merely
    /// looks like one.
    #[test]
    fn a_callout_marker_is_a_kind_an_optional_fold_and_a_title() {
        assert_eq!(
            marker("[!NOTE]"),
            Some(("note".into(), None, String::new()))
        );
        assert_eq!(marker("[!bug]"), Some(("bug".into(), None, String::new())));
        assert_eq!(
            marker("[!abstract] Summary"),
            Some(("abstract".into(), None, "Summary".into())),
        );
        assert_eq!(
            marker("[!note]- Folded"),
            Some(("note".into(), Some(false), "Folded".into())),
        );
        assert_eq!(
            marker("[!note]+ Open"),
            Some(("note".into(), Some(true), "Open".into())),
        );
        // A kind with a space in it is still a kind; Obsidian's vocabulary is open.
        assert_eq!(
            marker("[!my kind] T"),
            Some(("my kind".into(), None, "T".into())),
        );

        // And what is not a marker at all.
        assert_eq!(marker("Not a callout"), None);
        assert_eq!(marker("[!note"), None);
        assert_eq!(marker("[!]"), None);
        assert_eq!(marker("[!note]x"), None, "a kind runs to the bracket");
        assert_eq!(marker(" [!note]"), None, "a marker opens the line");
    }
}
