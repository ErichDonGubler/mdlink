use std::{
    collections::BTreeMap,
    fmt::{self, Display},
    sync::OnceLock,
};

use itertools::Itertools;
use joinery::JoinableIterator;
use lazy_format::{lazy_format, make_lazy_format};
use serde::Deserialize;
use url::Url;

use crate::{config::Layered, FancyMarkdownMatched};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    // TODO: v2 idea: familiar orgs, single focal repo
    #[serde(default)]
    orgs: BTreeMap<String, OrgEntry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct OrgEntry {
    unmatched_repo_prefix: Option<RepoPrefix>,
    #[serde(default)]
    repos: BTreeMap<String, RepoEntry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RepoEntry {
    prefix: Option<RepoPrefix>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepoPrefix {
    OrgAndRepo,
    RepoOnly,
    None,
}

pub(crate) fn try_write_markdown_url<'a>(
    config: Layered<&Config>,
    url: &'a Url,
    mut path_segments: impl Iterator<Item = &'a str> + Clone,
    mut f: impl fmt::Write,
) -> Result<Option<FancyMarkdownMatched>, fmt::Error> {
    let orgs = config.map(|github| &github.orgs);

    if let Some((org, repo)) = path_segments.next_tuple() {
        let prefix = {
            let repo_entry = orgs
                .clone()
                .inwards()
                .filter_map(|layer| layer.get(org))
                .filter_map(|org_entry| org_entry.repos.get(repo))
                .find_map(|repo| repo.prefix);
            let fallback_org_prefix = || {
                orgs.inwards()
                    .filter_map(|layer| layer.get(org))
                    .find_map(|org_entry| org_entry.unmatched_repo_prefix)
            };
            repo_entry
                .or_else(fallback_org_prefix)
                .unwrap_or(RepoPrefix::OrgAndRepo)
        };

        let backticked_org_and_repo = lazy_format!("`{org}/{repo}`");
        let backticked_org_and_repo: &dyn Display = &backticked_org_and_repo;
        let backticked_repo = lazy_format!("`{repo}`");
        let backticked_repo: &dyn Display = &backticked_repo;

        if path_segments.clone().next().is_none() {
            let display = match prefix {
                RepoPrefix::OrgAndRepo => backticked_org_and_repo,
                // We're pointing to a page about the repo, so it doesn't make sense to omit the
                // name.
                RepoPrefix::RepoOnly | RepoPrefix::None => backticked_repo,
            };
            write!(f, "[{display}]({url})")?;
            return Ok(Some(FancyMarkdownMatched::Yes));
        }
        let repo_prefix = match prefix {
            RepoPrefix::OrgAndRepo => backticked_org_and_repo,
            RepoPrefix::RepoOnly => backticked_repo,
            RepoPrefix::None => &"",
        };

        if let Some(("issues" | "pull", issue_num)) = path_segments.clone().next_tuple() {
            write!(f, "[{repo_prefix}#{issue_num}]({url})")?;
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
                    let commitish = lazy_format!("`{commitish}`");
                    let commit_ref = [repo_prefix, &commitish].into_iter().join_with(':');
                    write!(
                        f,
                        "[{commit_ref}:`{}`{}]({url})",
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
