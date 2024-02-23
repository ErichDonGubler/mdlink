//! Configuration for [`crate`].

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
};

use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};

/// Configuration to be passed to [`crate::try_write_markdown_url`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub general: ConfigLayer,
    #[serde(default)]
    pub profiles: BTreeMap<String, ConfigLayer>,
}

impl Config {
    /// Reads configuration (i.e., `config.toml`) from the application config. directory, dictated
    /// by the current platform's conventions.
    pub fn read_from_project_dir() -> Result<Self, ConfigReadError> {
        let project_dirs = directories::ProjectDirs::from("", "", "mdlink").unwrap();
        let config_dir = project_dirs.config_dir();
        log::trace!(
            "ensuring that config. directory is created at path {}",
            config_dir.display()
        );
        fs::create_dir_all(config_dir).context(CreateDirectorySnafu)?;
        let config_path = config_dir.join("config.toml");
        log::trace!(
            "ensuring that config. file is created at path {}",
            config_path.display()
        );
        let mut config_file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .read(true)
            .truncate(false)
            .open(&config_path)
            .context(OpenFileSnafu)?;
        let config_file_contents = {
            let mut buf = String::new();
            config_file
                .read_to_string(&mut buf)
                .context(ReadFileSnafu)?;
            buf
        };
        toml::from_str(&config_file_contents).context(DeserializeFileContentsAsTomlSnafu)
    }

    /// Fetch a field from this configuration's layers, using `f` as an extractor.
    ///
    /// The most specific layer(s) are tried first; if a `profile` is specified, then the
    /// `profiles` table is consulted first, falling back to the `general` configuration if `f`
    /// returns `None`.
    pub fn layers_from_profile(
        &self,
        profile: Option<&str>,
    ) -> Result<Layered<&ConfigLayer>, LayeredConfigError> {
        let Self { general, profiles } = self;
        Ok(Layered {
            general,
            profile: profile
                .map(|profile| profiles.get(profile).context(InvalidProfileNameSnafu))
                .transpose()?,
        })
    }
}

/// An error encountered with a call to [`Config::read_from_project_dir`].
#[derive(Debug, Snafu)]
pub enum ConfigReadError {
    #[snafu(display("failed to ensure that config. directory was created"))]
    CreateDirectory { source: io::Error },
    #[snafu(display("failed to ensure that config. file was created"))]
    CreateFile { source: io::Error },
    #[snafu(display("failed to open config. file"))]
    OpenFile { source: io::Error },
    #[snafu(display("failed to read config. file"))]
    ReadFile { source: io::Error },
    #[snafu(display("failed to deserialize config. file contents as TOML"))]
    DeserializeFileContentsAsToml { source: toml::de::Error },
}

/// A single layer of configuration supported by a [`Config`].
#[derive(Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigLayer {}

/// Layers of configuration applicable to a single profile selection. Can be created from
/// [`Config::layers_from_profile`].
#[derive(Clone, Debug)]
pub(crate) struct Layered<T> {
    pub general: T,
    pub profile: Option<T>,
}

impl<T> Layered<T> {
    /// Map the `T` of `Self` to `U` via `f`.
    #[must_use]
    pub(crate) fn map<U>(self, mut f: impl FnMut(T) -> U) -> Layered<U> {
        let Self { general, profile } = self;
        Layered {
            general: f(general),
            profile: profile.map(f),
        }
    }

    /// Iterate over layers in configuration, from most to least specific.
    #[must_use]
    pub(crate) fn inwards(self) -> impl Iterator<Item = T> {
        let Self { general, profile } = self;
        profile.into_iter().chain(Some(general))
    }
}

/// An error that may be encountered in a call to [`Config::layers_from_profile`].
#[derive(Debug, Snafu)]
pub enum LayeredConfigError {
    #[snafu(display("unrecognized profile name"))]
    InvalidProfileName,
}
