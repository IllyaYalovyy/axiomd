//! The only things a document is allowed to ask the application for.
//!
//! A rendered document cannot run a script, so the one way it can ask for anything
//! is a link the reader clicks. This module owns both ends of that: the URI the
//! pipeline writes into a placeholder card, and the reading of it back into a typed
//! request when the click arrives. Nothing else may spell either half, so the format
//! cannot drift between the renderer and the app.
//!
//! The vocabulary is deliberately tiny and closed. A URI that is not exactly one of
//! these is not a request, whoever wrote it — a hostile document forging one gets
//! the same answer an ordinary link gets.

/// The host requests live on. Never a document and never an asset, so a request
/// cannot be confused with either, and the scheme handler answers for none of them.
const HOST: &str = "axiomd://request/";

/// What a rendered document asks the application to do.
///
/// Both variants are the same user action seen twice: the reader pressed something
/// that says it will fetch a remote image, which is the only network use axiomd has
/// (`design_decisions.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Load the one remote image whose placeholder card was pressed.
    LoadImage(String),
    /// Load every remote image in the document that is still a placeholder.
    LoadAllImages,
}

impl Request {
    /// The URI a document links to in order to make this request.
    pub fn uri(&self) -> String {
        match self {
            Request::LoadImage(url) => format!("{HOST}image?src={}", encode(url)),
            Request::LoadAllImages => format!("{HOST}image?all"),
        }
    }

    /// The request `uri` names, or `None` when it names none.
    pub fn from_uri(uri: &str) -> Option<Request> {
        let query = uri.strip_prefix(HOST)?.strip_prefix("image?")?;
        match query {
            "all" => Some(Request::LoadAllImages),
            _ => Some(Request::LoadImage(decode(query.strip_prefix("src=")?)?)),
        }
    }
}

/// Percent-encodes everything but the unreserved set, so a URL travels through a
/// query parameter as data: its own `&`, `#`, `?` and spaces cannot end the query
/// early, and non-ASCII text arrives as the bytes it left as.
fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// The inverse of [`encode`]. `None` for anything that is not what it produced.
fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let digits = value.get(at + 1..at + 3)?;
            decoded.push(u8::from_str_radix(digits, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Where a remote URL would fetch from, as the reader needs to see it before
/// deciding: the host, without the scheme, the credentials or the path.
///
/// A reference with no host — a `data:` image — is named by its scheme instead, so
/// the card says what it is without unrolling a hundred kilobytes of base64 across
/// the reader's document.
pub(crate) fn origin_of(url: &str) -> &str {
    let Some(at) = url.find("//") else {
        return match url.find(':') {
            Some(colon) => &url[..colon + 1],
            None => url,
        };
    };
    let after_scheme = &url[at + 2..];
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // `user:password@host` — the reader is being shown where this goes, not who it
    // claims to be.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() { url } else { host }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_is_shown_the_host_an_image_would_come_from() {
        assert_eq!(
            origin_of("https://cdn.example.com/a/b.png"),
            "cdn.example.com"
        );
        assert_eq!(origin_of("http://example.com:8080/x"), "example.com:8080");
        assert_eq!(origin_of("//example.com/x.png"), "example.com");
        assert_eq!(origin_of("https://user:pw@evil.example/x"), "evil.example");
        assert_eq!(origin_of("data:image/png;base64,AAAA"), "data:");
        assert_eq!(origin_of("nonsense"), "nonsense");
    }

    /// The encoding is the whole reason a URL can be carried in a query at all.
    #[test]
    fn a_url_that_would_end_the_query_early_survives_it() {
        let hostile = "https://example.com/?a=1&b=2#frag src=other";
        assert_eq!(
            Request::from_uri(&Request::LoadImage(hostile.to_owned()).uri()),
            Some(Request::LoadImage(hostile.to_owned())),
        );
    }

    #[test]
    fn a_broken_encoding_is_not_a_request() {
        assert_eq!(Request::from_uri("axiomd://request/image?src=%zz"), None);
        assert_eq!(Request::from_uri("axiomd://request/image?src=%2"), None);
    }
}
