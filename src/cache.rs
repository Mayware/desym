use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

use crate::utils::{self, print_err};
use anyhow::{Result, anyhow};

pub struct Cache {
    pub created_files: Vec<String>,
    cache_path: PathBuf,
    json: Option<String>,
}

impl Cache {
    pub fn new(created_files: Vec<String>, cache_path: PathBuf) -> Self {
        Self {
            created_files: created_files,
            cache_path: cache_path,
            json: None,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Persistant {
    pub created_files: Vec<String>,
}

pub async fn read(path: Option<String>) -> Result<Cache> {
    let cache_dir = path
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| anyhow!("XDG_CACHE_HOME wasn't set, nor was HOME!"))?;
    let cache_path = cache_dir.join("desym.cache");

    if !cache_path.exists() {
        return Ok(Cache::new(Vec::new(), cache_path));
    }

    let text = fs::read_to_string(&cache_path).expect("Unable to read cache file");
    match serde_json::from_str::<Persistant>(&text) {
        Ok(persistant) => Ok(Cache::new(persistant.created_files, cache_path)),
        Err(err) => {
            print_err(
                &format!("Unable to read cache file! This may be due to a format update, or incorrect modification of the cache file: {}", err),
            ).await;
            let confirmation = utils::get_confirmation(
                "Would you like to continue? The cache file will be overwritten, and updated as needed",
            )
            .await;

            if confirmation {
                return Ok(Cache::new(Vec::new(), cache_path));
            } else {
                return Err(anyhow!(
                    "Unable to continue! Please rectify the cache file issue"
                ));
            }
        }
    }
}

impl Cache {
    pub fn store(&mut self, keys: Vec<String>) -> Result<()> {
        let persistant = Persistant {
            created_files: keys,
        };

        self.json = Some(serde_json::to_string_pretty(&persistant)?);
        Ok(())
    }

    pub async fn write(&mut self) -> Result<()> {
        tokio::fs::write(&self.cache_path, self.json.take().unwrap()).await?;
        Ok(())
    }
}
