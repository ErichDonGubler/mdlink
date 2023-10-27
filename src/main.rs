use std::{
    fmt::{self, Display},
    io::{self, stdin},
    sync::OnceLock,
};

use arboard::Clipboard;
use clap::Parser;
use format::lazy_format;
use itertools::Itertools;
use joinery::JoinableIterator;
use url::Url;

#[derive(Debug, Parser)]
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
            lazy_format!(move |f| {
                try_write_markdown_url(&url, &mut *f).and_then(|matched| match matched {
                    FancyMarkdownMatched::No => write!(f, "{url}"),
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
                        if let Some(("issues" | "pull", issue_num)) =
                            path_segments.clone().next_tuple()
                        {
                            write!(f, "[`{org}/{repo}`#{issue_num}]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }

                        {
                            let mut path_segments = path_segments.clone();
                            if let Some(("blob", commitish)) = path_segments.next_tuple() {
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
                                        let start =
                                            caps.name("start").map(|m| m.as_str()).expect(concat!(
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
                                    lazy_format!(|f| {
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
                    if let Some((id,)) = path_segments.collect_tuple() {
                        if id
                            .strip_prefix("D")
                            .map_or(false, |rest| rest.chars().all(|c| c.is_ascii_digit()))
                        {
                            write!(f, "[{id}]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
                    }
                }
                "docs.rs" => {
                    if let Some((crate_name, ver, crate_name2)) = path_segments.next_tuple() {
                        if crate_name == crate_name2 {
                            log::debug!("ignoring version {ver:?}");

                            let symbol_caps;
                            let symbol = match path_segments.next_back() {
                                Some("index.html") => None,
                                Some(symbol) => {
                                    static SYMBOL_RE: OnceLock<regex::Regex> = OnceLock::new();
                                    let symbol_re = SYMBOL_RE.get_or_init(|| {
                                        regex::Regex::new(concat!(
                                            "(constant|struct|fn|enum|trait|attr)",
                                            r"\.",
                                            r"(?P<ident>\w+)",
                                            r"\.html"
                                        ))
                                        .unwrap()
                                    });
                                    symbol_caps = symbol_re.captures(symbol);
                                    match symbol_caps.as_ref().map(|caps| &caps["ident"]) {
                                        None => return Ok(FancyMarkdownMatched::No),
                                        ident => ident,
                                    }
                                }
                                None => return Ok(FancyMarkdownMatched::No),
                            };

                            let fragment_caps;
                            let fragment = match url.fragment() {
                                None => None,
                                Some(fragment) => {
                                    static FRAGMENT_CAPS: OnceLock<regex::Regex> = OnceLock::new();
                                    let fragment_re = FRAGMENT_CAPS.get_or_init(|| {
                                        regex::Regex::new(concat!(
                                            "(tymethod|method)",
                                            r"\.",
                                            r"(?P<ident>\w+)",
                                        ))
                                        .unwrap()
                                    });
                                    fragment_caps = fragment_re.captures(fragment);
                                    fragment_caps.as_ref().map(|caps| &caps["ident"])
                                }
                            };

                            let module_path = Some(crate_name)
                                .into_iter()
                                .chain(path_segments)
                                .chain(symbol)
                                .chain(fragment)
                                .join_with("::");
                            write!(f, "[`{module_path}`]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
                        }
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
            comment = lazy_format!(move |f| write!(f, ", comment {comment_id}"));
            comment_display = &comment;
        }
    }
    write!(f, "[{prefix}{bug_id}{postfix}{comment_display}]({url})")
}
