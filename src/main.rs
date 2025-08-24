use std::{
    fmt::{self, Display},
    io::{self, stdin},
    sync::OnceLock,
};

use arboard::Clipboard;
use clap::Parser;
use itertools::Itertools;
use joinery::JoinableIterator;
use lazy_format::make_lazy_format;
use url::Url;

#[derive(Debug, Parser)]
#[clap(about, author, version)]
enum Cli {
    Clipboard,
    Stdin,
    Args { urls: Vec<Url> },
}

fn main() {
    env_logger::init();

    let buf;
    let urls: Box<dyn Iterator<Item = Url>> = match Cli::parse() {
        Cli::Clipboard => {
            buf = Clipboard::new().unwrap().get_text().unwrap();
            Box::new(line_iter(&buf))
        }
        Cli::Stdin => {
            buf = io::read_to_string(stdin().lock()).expect("failed to read `stdin`");
            Box::new(line_iter(&buf))
        }
        Cli::Args { urls } => Box::new(urls.into_iter()),
    };

    for url in urls {
        println!(
            "{}",
            make_lazy_format!(|f| {
                try_write_markdown_url(&url, &mut *f).and_then(|matched| match matched {
                    FancyMarkdownMatched::No => write!(f, "<{url}>"),
                    FancyMarkdownMatched::Yes => Ok(()),
                })
            })
        )
    }
}

#[allow(clippy::needless_lifetimes)]
fn line_iter<'a>(s: &'a str) -> impl Iterator<Item = Url> + 'a {
    s.lines().zip(1u64..).filter_map(|(line, idx)| {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        match line.parse() {
            Ok(url) => Some(url),
            Err(e) => {
                log::error!("failed to parse line {idx}: {e}. Original line: ({line:?})");
                None
            }
        }
    })
}

#[derive(Clone, Copy, Debug)]
enum FancyMarkdownMatched {
    Yes,
    No,
}

fn try_write_markdown_url(
    url: &Url,
    mut f: impl fmt::Write,
) -> Result<FancyMarkdownMatched, fmt::Error> {
    if let "http" | "https" = url.scheme() {
        if let Some(host) = url.host_str() {
            let mut path_segments = url
                .path_segments()
                .expect("got URL with host but no path segments iterator (!?)");
            match host {
                "github.com" => {
                    if let Some((org, repo)) = path_segments.next_tuple() {
                        if path_segments.clone().next().is_none() {
                            write!(f, "[`{org}/{repo}`]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                        if let Some(("issues" | "pull", issue_num)) =
                            path_segments.clone().next_tuple()
                        {
                            write!(f, "[`{org}/{repo}`#{issue_num}]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }

                        {
                            let mut path_segments = path_segments.clone();
                            match path_segments.next_tuple() {
                                Some(("blob", commitish)) => {
                                    enum LineNumberSpec<'a> {
                                        Single(&'a str),
                                        Range { start: &'a str, end: &'a str },
                                    }
                                    let file_path_segments = path_segments;
                                    let line_num_spec = url.fragment().and_then(|frag| {
                                        static LINE_NUM_SPEC_RE: OnceLock<regex::Regex> =
                                            OnceLock::new();
                                        let line_num_spec_re = LINE_NUM_SPEC_RE.get_or_init(|| {
                                            regex::Regex::new(concat!(
                                                r#"L(?P<start>\d+)"#,
                                                r#"(:?-L(?P<end>\d+))?"#,
                                            ))
                                            .unwrap()
                                        });
                                        line_num_spec_re.captures(frag).map(|caps| {
                                            let start = caps
                                                .name("start")
                                                .map(|m| m.as_str())
                                                .expect(concat!(
                                                    "matched line number spec. regex, ",
                                                    "but unconditional `start` capture not found"
                                                ));

                                            caps.name("end")
                                                .map(|m| m.as_str())
                                                .map(|end| LineNumberSpec::Range { start, end })
                                                .unwrap_or(LineNumberSpec::Single(start))
                                        })
                                    });
                                    write!(
                                        f,
                                        "[`{org}/{repo}`:`{commitish}`:`{}`{}]({url})",
                                        file_path_segments.join_with('/'),
                                        make_lazy_format!(|f| {
                                            match line_num_spec {
                                                Some(LineNumberSpec::Single(num)) => {
                                                    write!(f, ":{num}")
                                                }
                                                Some(LineNumberSpec::Range { start, end }) => {
                                                    write!(f, ":{start}-{end}")
                                                }
                                                None => Ok(()),
                                            }
                                        })
                                    )?;
                                    return Ok(FancyMarkdownMatched::Yes);
                                }
                                Some(("commit", commitish)) => {
                                    if path_segments.clone().next().is_none() {
                                        write!(f, "[`{org}/{repo}`:`{commitish}`]({url})")?;
                                        return Ok(FancyMarkdownMatched::Yes);
                                    }

                                    let file_path_segments = path_segments;
                                    write!(
                                        f,
                                        "[`{org}/{repo}`:`{commitish}`:`{}`]({url})",
                                        file_path_segments.join_with('/'),
                                    )?;
                                    return Ok(FancyMarkdownMatched::Yes);
                                }
                                Some(("releases", "tag")) => {
                                    if let Some(tag) = path_segments.next() {
                                        match (path_segments.next(), path_segments.next()) {
                                            (Some(""), None) | (None, ..) => {
                                                static COMPONENT_VERSION_RE: OnceLock<
                                                    regex::Regex,
                                                > = OnceLock::new();

                                                if let Some(captures) = COMPONENT_VERSION_RE
                                                    .get_or_init(|| {
                                                        regex::Regex::new(concat!(
                                                            r"(?P<component>.+)",
                                                            "-",
                                                            r"(?P<version>v\d+(:?\.\d+){0,2})"
                                                        ))
                                                        .unwrap()
                                                    })
                                                    .captures(tag)
                                                {
                                                    let component = &captures["component"];
                                                    let version = &captures["version"];
                                                    write!(f, "[`{component}` {version}]({url})")?;
                                                } else {
                                                    write!(f, "[`{tag}` tag release]({url})")?;
                                                }
                                                return Ok(FancyMarkdownMatched::Yes);
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                _ => (),
                            }
                        }
                    }
                }
                "bugzil.la" => {
                    if let Some((bug_id,)) = path_segments.collect_tuple() {
                        render_bugzilla(url, bug_id, f)?;
                        return Ok(FancyMarkdownMatched::Yes);
                    }
                }
                "bugzilla.mozilla.org" => {
                    if let Some("show_bug.cgi") = path_segments.next() {
                        if path_segments.next().is_none() {
                            if let Some(bug_id) = url
                                .query_pairs()
                                .find_map(|(k, v)| (k == "id").then_some(v))
                            {
                                render_bugzilla(url, bug_id.as_ref(), f)?;
                                return Ok(FancyMarkdownMatched::Yes);
                            }
                        }
                    }
                }
                "phabricator.services.mozilla.com" => {
                    if let Some(("differential", "diff", diff_id)) =
                        path_segments.clone().next_tuple()
                    {
                        if let Some(("",)) = path_segments.collect_tuple() {
                            // extra slash at end, ignore it
                        }
                        write!(f, "[diff {diff_id}]({url})")?;
                        return Ok(FancyMarkdownMatched::Yes);
                    } else if let Some((id,)) = path_segments.collect_tuple() {
                        if id
                            .strip_prefix('D')
                            .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
                        {
                            write!(f, "[{id}]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                    }
                }
                "crates.io" => {
                    if let Some(("crates", crate_name, crate_version)) =
                        path_segments.collect_tuple()
                    {
                        write!(f, "[`{crate_name}` v{crate_version}]({url})")?;
                        return Ok(FancyMarkdownMatched::Yes);
                    }
                }
                "docs.rs" => {
                    if let Some((crate_pkg_name, ver, crate_module_name)) =
                        path_segments.next_tuple()
                    {
                        if crate_pkg_name.replace('-', "_") == crate_module_name {
                            log::debug!("ignoring version {ver:?}");

                            let mut symbol_caps = None;
                            let mut fragment_caps = None;
                            match extract_rust_symbol_path(
                                crate_module_name,
                                path_segments,
                                url.fragment(),
                                &mut symbol_caps,
                                &mut fragment_caps,
                            ) {
                                Some(module_path) => {
                                    write!(f, "[`{module_path}`]({url})")?;
                                    return Ok(FancyMarkdownMatched::Yes);
                                }
                                None => return Ok(FancyMarkdownMatched::No),
                            };
                        }
                    }
                }
                "doc.rust-lang.org" => {
                    if let Some(
                        (
                            "stable" | "beta" | "nightly",
                            crate_module_name @ ("core" | "alloc" | "std"),
                        )
                        | (crate_module_name @ ("core" | "alloc" | "std"), _),
                    ) = path_segments.next_tuple()
                    {
                        let mut symbol_caps = None;
                        let mut fragment_caps = None;
                        match extract_rust_symbol_path(
                            crate_module_name,
                            path_segments,
                            url.fragment(),
                            &mut symbol_caps,
                            &mut fragment_caps,
                        ) {
                            Some(module_path) => {
                                write!(f, "[`{module_path}`]({url})")?;
                                return Ok(FancyMarkdownMatched::Yes);
                            }
                            None => return Ok(FancyMarkdownMatched::No),
                        };
                    }
                }
                "rust-lang.github.io" => {
                    if let Some(("rust-clippy", release_stage, "index.html")) =
                        path_segments.collect_tuple()
                    {
                        if matches!(release_stage, "stable" | "beta" | "nightly") {
                            if let Some(term) = url.fragment() {
                                if let Some(term) = term.strip_prefix('/') {
                                    write!(f, "[search for `{term}` in `{release_stage}`]({url})")?;
                                } else {
                                    let lint_name = term;
                                    write!(
                                        f,
                                        "[`clippy::{lint_name}` in `{release_stage}`]({url})"
                                    )?;
                                }
                            } else {
                                write!(f, "[`clippy` lints in `{release_stage}`]({url})")?;
                            }
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                    }
                }
                "searchfox.org" => {
                    let is_moz_central = path_segments
                        .next()
                        .filter(|repo| repo == &"mozilla-central" || repo == &"firefox-main")
                        .and_then(|_repo| path_segments.next())
                        .is_some_and(|history| match history {
                            "source" => true,
                            "rev" => {
                                let _rev_hash = path_segments.next();
                                true
                            }
                            _ => false,
                        });
                    if is_moz_central {
                        let file_path = path_segments.join_with('/');
                        let line_range_probably = make_lazy_format!(|f| {
                            if let Some(fragment) = url.fragment() {
                                write!(f, "{fragment}")?;
                            }
                            Ok(())
                        });
                        write!(f, "[`{file_path}`:{line_range_probably}]({url})")?;
                        return Ok(FancyMarkdownMatched::Yes);
                    }
                }
                "treeherder.mozilla.org" => {
                    if let Some(("jobs",)) = path_segments.collect_tuple() {
                        let mut repo = None;
                        let mut revision = None;
                        for (key, value) in url.query_pairs() {
                            match key.as_ref() {
                                "repo" => repo = repo.or(Some(value)),
                                "revision" => revision = revision.or(Some(value)),
                                _ => (),
                            }
                        }

                        if let (Some(repo), Some(revision)) = (repo, revision) {
                            let revision = revision.get(..12).unwrap_or(revision.as_ref());
                            write!(f, "[`{repo}:{revision}`]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                    }
                }
                "gpuweb.github.io" => {
                    if let Some(("cts", "standalone", "")) = path_segments.collect_tuple() {
                        if let Some(test_path) =
                            url.query_pairs().find_map(|(k, v)| (k == "q").then_some(v))
                        {
                            write!(f, "[`{test_path}`]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                    }
                }
                "hg.mozilla.org" | "hg-edge.mozilla.org" => {
                    // https://hg-edge.mozilla.org/mozilla-central/rev/f956d7e03a822a09fbb84e9b474db8e0167095f1
                    if let Some((repo, "rev", hash)) = path_segments.collect_tuple() {
                        write!(f, "[`{repo}`:`{hash}`]({url})")?;
                        return Ok(FancyMarkdownMatched::Yes);
                    }
                }
                _ => (),
            }
        }
    }

    Ok(FancyMarkdownMatched::No)
}

fn render_bugzilla(url: &Url, bug_id: &str, mut f: impl fmt::Write) -> fmt::Result {
    let (prefix, postfix) = if bug_id.chars().all(|c| c.is_ascii_digit()) {
        ("bug ", "")
    } else {
        ("`", "`")
    };

    let comment;
    let mut comment_display: &dyn Display = &"";

    if let Some(fragment) = url.fragment() {
        if let Some(("", comment_id)) = fragment.split_once('c') {
            comment = make_lazy_format!(|f| write!(f, ", comment {comment_id}"));
            comment_display = &comment;
        }
    }
    write!(f, "[{prefix}{bug_id}{postfix}{comment_display}]({url})")
}

fn extract_rust_symbol_path<'a>(
    crate_module_name: &'a str,
    mut path_segments: impl Clone + DoubleEndedIterator<Item = &'a str> + 'a,
    fragment: Option<&'a str>,
    symbol_caps: &'a mut Option<regex::Captures<'a>>,
    fragment_caps: &'a mut Option<regex::Captures<'a>>,
) -> Option<impl Clone + Display + 'a> {
    let symbol = match path_segments.next_back() {
        Some("index.html" | "") | None => None,
        Some(symbol) => {
            static SYMBOL_RE: OnceLock<regex::Regex> = OnceLock::new();
            let symbol_re = SYMBOL_RE.get_or_init(|| {
                regex::Regex::new(concat!(
                    "(?P<symbol_kind>constant|struct|fn|enum|trait|attr|primitive|type)",
                    r"\.",
                    r"(?P<ident>\w+)",
                    r"\.html"
                ))
                .unwrap()
            });
            *symbol_caps = symbol_re.captures(symbol);
            match symbol_caps
                .as_ref()
                .map(|caps| (&caps["symbol_kind"], &caps["ident"]))
            {
                None => return None,
                some => some,
            }
        }
    };

    let fragment = match fragment {
        None => None,
        Some(fragment) => {
            static FRAGMENT_CAPS: OnceLock<regex::Regex> = OnceLock::new();
            let fragment_re = FRAGMENT_CAPS.get_or_init(|| {
                regex::Regex::new(concat!(
                    "(tymethod|method|associatedconstant|structfield)",
                    r"\.",
                    r"(?P<ident>\w+)"
                ))
                .unwrap()
            });
            *fragment_caps = fragment_re.captures(fragment);
            fragment_caps.as_ref().map(|caps| &caps["ident"])
        }
    };

    let mut symbol_name = None;
    let mut crate_module_name = Some(crate_module_name);
    if let Some((kind, name)) = symbol {
        symbol_name = Some(name);
        if kind == "primitive" {
            crate_module_name = None
        }
    }

    Some(
        crate_module_name
            .into_iter()
            .chain(path_segments)
            .chain(symbol_name)
            .chain(fragment)
            .join_with("::"),
    )
}
