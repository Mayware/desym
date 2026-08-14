mod cache;
mod files;
mod input;
mod symlinks;
mod utils;

use anyhow::{Result, anyhow};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::HashMap, os::unix::fs::MetadataExt, path::Path, process::ExitCode};

// For values that are expected, but can sometimes be omitted (like uid, gid and mode for symlinks),
// we just use sentinel values, because otherwise with optionals we'd need to deal with pointless
// unwrapping later in the code, the code that parses this should handle resolving defaults
#[derive(Debug, Deserialize, JsonSchema)]
struct Entry {
    source: String,
    #[serde(default = "default_max")]
    uid: u32,
    #[serde(default = "default_max")]
    gid: u32,
    #[serde(default = "default_max")]
    mode: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Config {
    #[serde(default)]
    files: HashMap<String, Entry>,
    #[serde(default)]
    symlinks: HashMap<String, Entry>,
    #[serde(default)]
    settings: Settings,
}

#[derive(Debug, Default, Deserialize, JsonSchema, Clone)]
struct Settings {
    keep_clobbered: Option<bool>,
    cache_path: Option<String>,
}

fn default_max() -> u32 {
    u32::MAX
}

impl Entry {
    // Inherit the uid/gid/mode of the source file, if it was left unset
    pub async fn resolve_defaults(&mut self) -> Result<()> {
        if self.uid == u32::MAX || self.gid == u32::MAX || self.mode == u32::MAX {
            let metadata = match tokio::fs::symlink_metadata(&self.source).await {
                Ok(metadata) => metadata,
                Err(err) => {
                    return Err(anyhow!(
                        "Failed to read metadata for resolve defaults for \n{}\nThe following fields were not specified: [{}]: {}\n{}\n{}",
                        self.source,
                        [("uid", self.uid), ("gid", self.gid), ("mode", self.mode),]
                            .iter()
                            .filter(|(_, value)| *value == u32::MAX)
                            .map(|(name, _)| *name)
                            .collect::<Vec<_>>()
                            .join(", "),
                        err,
                        "Commonly, this is for when creating an arbitrary files, since it is not possible to infer existing permissions.",
                        "Please manually specify the required fields."
                    ));
                }
            };

            if self.uid == u32::MAX {
                self.uid = metadata.uid();
            }
            if self.gid == u32::MAX {
                self.gid = metadata.gid();
            }
            if self.mode == u32::MAX {
                self.mode = metadata.mode();
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = async {
        let path = std::env::args().nth(1).expect("Configuration path not given");
        let Config {
            mut files,
            mut symlinks,
            mut settings,
        } = serde_json::from_str(&std::fs::read_to_string(path)?).expect("Failed to parse config");
        let mut cache = cache::read(settings.cache_path.take()).await?;
        utils::init_settings(settings);

        // Clean up no longer referenced files in the config, that we cached as having been made
        for path in cache.created_files.iter().filter(|f| !files.contains_key(*f) && !symlinks.contains_key(*f)) {
            // Only remove the path, if it still exists
            if Path::new(path).exists() {
                utils::remove_path(std::path::Path::new(path)).await?;
            }
        }

        // Store the file paths we will have made, if the main body is successful
        let keys: Vec<String> = files.keys().chain(symlinks.keys()).cloned().collect();
        cache.store(keys)?;

        // Fill in default values for entries that were not entirely set
        let results = files.values_mut().chain(symlinks.values_mut()).map(|entry| entry.resolve_defaults()).collect::<Vec<_>>();
        futures::future::try_join_all(results).await?;

        // Create / delete paths as necessary for each core
        tokio::try_join!(symlinks::process(symlinks), files::process(files))?;

        // Write the new file paths we made to the cache file
        cache.write().await?;

        Ok::<(), anyhow::Error>(())
    }
    .await;

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            utils::print_real_err(&err).await;
            ExitCode::FAILURE
        }
    }
}
