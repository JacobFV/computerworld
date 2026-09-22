//! The address bar is an omnibox: it takes what a person typed, not a URL.
//!
//! [`omnibox`] reads a line of text the way Chrome's does, in this order:
//!
//! 1. A leading `?` forces a search, whatever the rest looks like.
//! 2. An explicit scheme (`http://`, `https://`, and anything else written
//!    `scheme://` or `scheme:`) is honoured as typed, so a programmatic caller that
//!    passes a bad URL is told so rather than silently searched for.
//! 3. Text shaped like a host — a dotted name ending in a TLD-shaped label, an IP
//!    literal, a host with an explicit port, or a single label — becomes `https://`
//!    plus the text. The name still has to resolve: when it does not, what was typed
//!    was never an address, and it becomes a search after all ([`Typed::Host`]).
//! 4. Anything else is a search on the browser's default engine.
//!
//! A relative reference (`/path`, `./x`, `#frag`) is left to the URL parser so that
//! programmatic navigation against the page on show keeps working; nobody types one
//! into an address bar.
use std::net::{Ipv4Addr, Ipv6Addr};

/// Where a browser sends omnibox searches unless its world says otherwise. `%s`
/// stands for the query.
pub const DEFAULT_SEARCH_ENGINE: &str = "https://google.com/search?q=%s";

/// What a line of typed text turned out to mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    /// A URL, to be navigated to as it stands. Failures are the caller's to see.
    Url(String),
    /// Shaped like a host: `url` is tried first, and `search` replaces it when the
    /// name does not resolve.
    Host { url: String, search: String },
    /// Not an address at all: the search engine's results page for it.
    Search(String),
}

/// `engine` with the query substituted for `%s`, urlencoded; a template with no `%s`
/// has the query appended, so a bare `https://example.test/find?q=` works too.
pub fn search_url(engine: &str, query: &str) -> String {
    let engine = if engine.trim().is_empty() {
        DEFAULT_SEARCH_ENGINE
    } else {
        engine
    };
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    if engine.contains("%s") {
        engine.replace("%s", &encoded)
    } else {
        format!("{engine}{encoded}")
    }
}

/// Read a line of typed text as an address bar does. See the module docs for the
/// order the rules are applied in.
pub fn omnibox(text: &str, engine: &str) -> Typed {
    let text = text.trim();
    // `?rust` searches for `rust` even though `rust` alone would be a host to try.
    if let Some(query) = text.strip_prefix('?') {
        return Typed::Search(search_url(engine, query.trim_start()));
    }
    // `scheme://…` is a URL whatever else it contains, including spaces: a caller
    // that wrote one meant it, and deserves the parser's complaint.
    if hierarchical_scheme(text) {
        return Typed::Url(text.to_owned());
    }
    // Nothing else with a space in it is an address.
    if text.is_empty() || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return if text.is_empty() {
            // An empty address bar reloads the page on show, as it always has.
            Typed::Url(String::new())
        } else {
            Typed::Search(search_url(engine, text))
        };
    }
    // `mailto:`, `javascript:`, `file:` without slashes: still a scheme, still the
    // caller's to hear about.
    if opaque_scheme(text) || relative_reference(text) {
        return Typed::Url(text.to_owned());
    }
    let authority = text.split(['/', '?', '#']).next().unwrap_or(text);
    if host_shaped(authority) {
        return Typed::Host {
            url: format!("https://{text}"),
            search: search_url(engine, text),
        };
    }
    Typed::Search(search_url(engine, text))
}

/// `scheme://rest`.
fn hierarchical_scheme(text: &str) -> bool {
    text.split_once("://")
        .is_some_and(|(scheme, _)| is_scheme(scheme))
}

/// The schemes a browser knows without a `//` after them. Everything else before a
/// colon is just text: `localhost:8080` is a host and a port, `time:noon` is a
/// search, and neither is a scheme.
const OPAQUE_SCHEMES: [&str; 10] = [
    "about",
    "blob",
    "data",
    "file",
    "ftp",
    "javascript",
    "mailto",
    "sms",
    "tel",
    "view-source",
];

fn opaque_scheme(text: &str) -> bool {
    text.split_once(':').is_some_and(|(scheme, _)| {
        OPAQUE_SCHEMES
            .iter()
            .any(|known| scheme.eq_ignore_ascii_case(known))
    })
}

fn is_scheme(scheme: &str) -> bool {
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn relative_reference(text: &str) -> bool {
    text.starts_with('/')
        || text.starts_with("./")
        || text.starts_with("../")
        || text.starts_with('#')
}

/// Whether `authority` (everything before the first `/`, `?` or `#`) reads as a host.
/// An honest, simple test: an IP literal, a host with an explicit port, a dotted name
/// whose last label is shaped like a TLD, or a single label — which is only really a
/// host if the world's DNS says so, and that is what [`Typed::Host`] goes and asks.
fn host_shaped(authority: &str) -> bool {
    // `user@host` is an email address to everyone who types one.
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (inner, port) = match rest.split_once(']') {
            Some((inner, port)) => (inner, port),
            None => return false,
        };
        return inner.parse::<Ipv6Addr>().is_ok() && (port.is_empty() || is_port(&port[1..]));
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if is_port(port) => host,
        Some(_) => return false,
        None => authority,
    };
    if host.is_empty() {
        return false;
    }
    if host.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if !labels.iter().all(|label| is_label(label)) {
        return false;
    }
    match labels.as_slice() {
        // `localhost`, `intranet`: a name only DNS can confirm.
        [_] => true,
        // `github.com`, `foo.bar`, but not `3.14` or `v1.2`.
        [.., tld] => tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()),
        [] => false,
    }
}

fn is_port(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.chars().all(|c| c.is_ascii_digit())
}

fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENGINE: &str = DEFAULT_SEARCH_ENGINE;
    fn typed(text: &str) -> Typed {
        omnibox(text, ENGINE)
    }
    fn url(text: &str) -> String {
        match typed(text) {
            Typed::Url(url) => url,
            other => panic!("{text} is not a plain URL: {other:?}"),
        }
    }
    fn host(text: &str) -> String {
        match typed(text) {
            Typed::Host { url, .. } => url,
            other => panic!("{text} is not a host: {other:?}"),
        }
    }
    fn search(text: &str) -> String {
        match typed(text) {
            Typed::Search(url) => url,
            Typed::Host { search, .. } => panic!("{text} is a host first ({search})"),
            other => panic!("{text} is not a search: {other:?}"),
        }
    }

    #[test]
    fn an_explicit_scheme_is_honoured_as_typed() {
        assert_eq!(url("https://github.com/"), "https://github.com/");
        assert_eq!(url("http://intranet.internal"), "http://intranet.internal");
        // Not ours to serve, but ours to complain about rather than search for.
        assert_eq!(url("file:///etc/passwd"), "file:///etc/passwd");
        assert_eq!(
            url("mailto:alice@northwind.example"),
            "mailto:alice@northwind.example"
        );
        assert_eq!(url("javascript:alert(1)"), "javascript:alert(1)");
        // A caller that wrote a scheme meant a URL, even a malformed one.
        assert_eq!(url("https://two words"), "https://two words");
        // Surrounding space is trimmed the way a paste into the bar is.
        assert_eq!(url("  https://github.com/  "), "https://github.com/");
    }

    #[test]
    fn host_shaped_text_gets_https() {
        assert_eq!(host("github.com"), "https://github.com");
        assert_eq!(
            host("github.com/northstar/atlas"),
            "https://github.com/northstar/atlas"
        );
        assert_eq!(host("intranet.internal"), "https://intranet.internal");
        assert_eq!(
            host("mail.google.com/threads/3"),
            "https://mail.google.com/threads/3"
        );
        assert_eq!(host("10.0.1.10"), "https://10.0.1.10");
        assert_eq!(
            host("10.0.1.10:8080/health"),
            "https://10.0.1.10:8080/health"
        );
        assert_eq!(host("localhost:8080"), "https://localhost:8080");
        assert_eq!(host("localhost:3000"), "https://localhost:3000");
        assert_eq!(host("[::1]:8080"), "https://[::1]:8080");
        // A single label is a host candidate: only DNS can settle it.
        assert_eq!(host("intranet"), "https://intranet");
        assert_eq!(host("localhost"), "https://localhost");
        // `foo.bar` is shaped like a host; `foo bar` is two words.
        assert_eq!(host("foo.bar"), "https://foo.bar");
        assert_eq!(search("foo bar"), "https://google.com/search?q=foo+bar");
    }

    #[test]
    fn a_host_carries_the_search_it_falls_back_to() {
        assert_eq!(
            typed("github.com"),
            Typed::Host {
                url: "https://github.com".into(),
                search: "https://google.com/search?q=github.com".into(),
            }
        );
    }

    #[test]
    fn anything_that_is_not_an_address_is_searched_for() {
        assert_eq!(
            search("deterministic simulation"),
            "https://google.com/search?q=deterministic+simulation"
        );
        assert_eq!(
            search("what is a \"deterministic\" world? c++ & rust!"),
            "https://google.com/search?q=what+is+a+%22deterministic%22+world%3F+c%2B%2B+%26+rust%21"
        );
        // Numbers with dots are not hosts: no TLD-shaped last label.
        assert_eq!(search("3.14"), "https://google.com/search?q=3.14");
        assert_eq!(search("v1.2"), "https://google.com/search?q=v1.2");
        // An email address is not a site.
        assert_eq!(
            search("alice@northwind.example"),
            "https://google.com/search?q=alice%40northwind.example"
        );
        assert_eq!(search("1+1"), "https://google.com/search?q=1%2B1");
        assert_eq!(
            search("rust: the book"),
            "https://google.com/search?q=rust%3A+the+book"
        );
        // A port that is not a port.
        assert_eq!(
            search("time:noon"),
            "https://google.com/search?q=time%3Anoon"
        );
    }

    #[test]
    fn a_leading_question_mark_forces_a_search() {
        assert_eq!(
            search("?github.com"),
            "https://google.com/search?q=github.com"
        );
        assert_eq!(
            search("? github.com"),
            "https://google.com/search?q=github.com"
        );
        assert_eq!(
            search("?https://github.com"),
            "https://google.com/search?q=https%3A%2F%2Fgithub.com"
        );
    }

    #[test]
    fn relative_references_stay_relative_and_nothing_stays_nothing() {
        assert_eq!(url("/search?q=atlas"), "/search?q=atlas");
        assert_eq!(url("./next"), "./next");
        assert_eq!(url("../up"), "../up");
        assert_eq!(url("#section"), "#section");
        assert_eq!(url("   "), "");
    }

    #[test]
    fn the_engine_is_a_template() {
        assert_eq!(
            search_url("https://duckduckgo.com/?q=%s", "atlas ranking"),
            "https://duckduckgo.com/?q=atlas+ranking"
        );
        // A template without `%s` takes the query on the end.
        assert_eq!(
            search_url("https://bing.com/search?q=", "atlas"),
            "https://bing.com/search?q=atlas"
        );
        // An empty engine is the default rather than a broken URL.
        assert_eq!(search_url("", "atlas"), "https://google.com/search?q=atlas");
        assert_eq!(
            omnibox("two words", "https://bing.com/search?q=%s"),
            Typed::Search("https://bing.com/search?q=two+words".into())
        );
    }
}
