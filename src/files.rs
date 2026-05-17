use std::{collections::HashMap, fs, os::unix::fs::MetadataExt, path::Path};

use crate::{
    Entry,
    utils::{self, print_suc},
};

use anyhow::{Result, anyhow};
use tokio::task::JoinSet;

async fn write_file(path: &Path, content: &[u8], uid: u32, gid: u32, mode: u32) -> Result<()> {
    match fs::write(path, content) {
        Ok(_) => {
            utils::raw_chown(path, uid, gid).await?;
            utils::raw_chmod(path, mode).await?;

            print_suc(format!("Wrote to, Path: {}", path.display()).as_str()).await;
            Ok(())
        }
        Err(err) => {
            return Err(anyhow!(format!(
                "Failed to write to file, Path: {}, Error: {}",
                path.display(),
                err
            )));
        }
    }
}

pub async fn process(files: HashMap<String, Entry>) -> Result<()> {
    let mut set: JoinSet<Result<()>> = JoinSet::new();

    for (file_path, entry) in files {
        set.spawn(async move {
            let file_path = Path::new(&file_path);
            if let Some(metadata) =
                utils::get_matching_metadata(file_path, entry.uid, entry.gid).await?
            {
                if metadata.is_file() {
                    let file_content = tokio::fs::read(file_path).await?;

                    // No-op if everything matches
                    if entry.source.as_bytes() == file_content
                        && metadata.uid() == entry.uid
                        && metadata.gid() == entry.gid
                        && metadata.mode() == entry.mode
                    {
                        return Ok(());
                    }
                }

                utils::remove_path(file_path).await?;
            }

            write_file(file_path, entry.source.as_bytes(), entry.uid, entry.gid, entry.mode).await?;
            Ok(())
        });
    }

    while let Some(result) = set.join_next().await {
        result??;
    }

    Ok(())
}
