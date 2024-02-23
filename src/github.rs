use std::{fmt, sync::OnceLock};

use itertools::Itertools;
use joinery::JoinableIterator;
use lazy_format::make_lazy_format;
use serde::Deserialize;
use url::Url;

use crate::{config::Layered, FancyMarkdownMatched};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {}

pub(crate) fn try_write_markdown_url<'a>(
    config: Layered<&Config>,
    url: &'a Url,
    mut path_segments: impl Iterator<Item = &'a str> + Clone,
    mut f: impl fmt::Write,
) -> Result<Option<FancyMarkdownMatched>, fmt::Error> {
    let _ = config.map(|Config {}| ());

    if let Some((org, repo)) = path_segments.next_tuple() {
        if path_segments.clone().next().is_none() {
            write!(f, "[`{org}/{repo}`]({url})")?;
            return Ok(Some(FancyMarkdownMatched::Yes));
        }
        if let Some(("issues" | "pull", issue_num)) = path_segments.clone().next_tuple() {
            write!(f, "[`{org}/{repo}`#{issue_num}]({url})")?;
            return Ok(Some(FancyMarkdownMatched::Yes));
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
                        static LINE_NUM_SPEC_RE: OnceLock<regex::Regex> = OnceLock::new();
                        let line_num_spec_re = LINE_NUM_SPEC_RE.get_or_init(|| {
                            regex::Regex::new(concat!(
                                r#"L(?P<start>\d+)"#,
                                r#"(:?-L(?P<end>\d+))?"#,
                            ))
                            .unwrap()
                        });
                        line_num_spec_re.captures(frag).map(|caps| {
                            let start = caps.name("start").map(|m| m.as_str()).expect(concat!(
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
                    return Ok(Some(FancyMarkdownMatched::Yes));
                }
                Some(("commit", commitish)) => {
                    if path_segments.clone().next().is_none() {
                        write!(f, "[`{org}/{repo}`:`{commitish}`]({url})")?;
                        return Ok(Some(FancyMarkdownMatched::Yes));
                    }

                    let file_path_segments = path_segments;
                    write!(
                        f,
                        "[`{org}/{repo}`:`{commitish}`:`{}`]({url})",
                        file_path_segments.join_with('/'),
                    )?;
                    return Ok(Some(FancyMarkdownMatched::Yes));
                }
                Some(("releases", "tag")) => {
                    if let Some(tag) = path_segments.next() {
                        match (path_segments.next(), path_segments.next()) {
                            (Some(""), None) | (None, ..) => {
                                static COMPONENT_VERSION_RE: OnceLock<regex::Regex> =
                                    OnceLock::new();

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
                                return Ok(Some(FancyMarkdownMatched::Yes));
                            }
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
        }
    }

    Ok(None)
}
