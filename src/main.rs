use std::{
    fmt::{self, Display},
    io::{self, stdin},
};

use arboard::Clipboard;
use clap::Parser;
use format::lazy_format;
use itertools::Itertools;
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
                .expect("got URL with host but no path segments (!?)");
            match host {
                "github.com" => {
                    if let Some((org, repo)) = path_segments.next_tuple() {
                        if let Some(("issues" | "pull", issue_num)) =
                            path_segments.clone().next_tuple()
                        {
                            write!(f, "[`{org}/{repo}`#{issue_num}]({url})")?;
                            return Ok(FancyMarkdownMatched::Yes);
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
