use std::{
    fmt,
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
            #[allow(clippy::single_match)]
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
                _ => (),
            }
        }
    }

    Ok(FancyMarkdownMatched::No)
}
