#![feature(set_permissions_nofollow)]

mod cache;
mod files;
mod input;
mod symlinks;
mod utils;

use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::HashMap, env, os::unix::fs::MetadataExt};

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

#[derive(Default, Debug, Deserialize, JsonSchema, Clone)]
struct Settings {
    #[serde(default = "default_true")]
    add_path_confirmation: bool,
    #[serde(default = "default_true")]
    remove_path_confirmation: bool,
    cache_path: Option<String>,
}
fn default_true() -> bool {
    true
}
fn default_max() -> u32 {
    u32::MAX
}

impl Entry {
    // Inherit the uid/gid/mode of the source file, if it was left unset
    pub async fn resolve_defaults(&mut self) -> Result<()> {
        if self.uid == u32::MAX || self.gid == u32::MAX || self.mode == u32::MAX {
            let metadata = tokio::fs::symlink_metadata(&self.source).await?;

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

// Realistically we only need one real thread, more would be bloat
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = async {
        let arg = env::args().nth(1).expect("Failed to get json arg");
        let Config {
            mut files,
            mut symlinks,
            mut settings,
        } = serde_json::from_str(&arg).expect("Failed to parse config");
        let mut cache = cache::read(settings.cache_path.take()).await?;
        utils::init_settings(settings);

        // Clean up no longer referenced files in the config, that we cached as having been made
        for path in cache
            .created_files
            .iter()
            .filter(|f| !files.contains_key(*f) && !symlinks.contains_key(*f))
        {
            utils::remove_path(std::path::Path::new(path)).await?;
        }

        // Store the file paths we will have made, if the main body is successful
        let keys: Vec<String> = files.keys().chain(symlinks.keys()).cloned().collect();
        cache.store(keys)?;

        // Fill in default values for entries that were not entirely set
        let results = files
            .values_mut()
            .chain(symlinks.values_mut())
            .map(|entry| entry.resolve_defaults())
            .collect::<Vec<_>>();
        futures::future::try_join_all(results).await?;

        // Create / delete paths as necessary for each core
        tokio::try_join!(symlinks::process(symlinks), files::process(files))?;

        // Write the new file paths we made to the cache file
        cache.write().await?;

        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(err) = result {
        utils::print_err(err.to_string().as_str()).await;
    }
}
