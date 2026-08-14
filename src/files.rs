use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::{
    Entry,
    utils::{self, print_suc},
};

use anyhow::{Result, anyhow};
use tokio::task::JoinSet;

pub async fn process(files: HashMap<String, Entry>) -> Result<()> {
    let mut set: JoinSet<Result<()>> = JoinSet::new();

    for (file_path, entry) in files {
        set.spawn(async move {
            let file_path = Path::new(&file_path);
            let metadata = utils::get_metadata(file_path).await?;
            if let Some(metadata) = &metadata {
                if metadata.is_file() {
                    let file_content = tokio::fs::read(file_path).await?;

                    // No-op if everything matches
                    if entry.source.as_bytes() == file_content
                        && metadata.uid() == entry.uid
                        && metadata.gid() == entry.gid
                        // Only check permission bits
                        && (metadata.mode() & 0o777) == entry.mode
                    {
                        return Ok(());
                    }
                }
            }

            let created_paths = utils::require_parent_paths(file_path, entry.uid, entry.gid).await?;
            let limbo_path = PathBuf::from(utils::get_limbo(file_path.to_string_lossy()));
            if let Err(err) = async {
                // A failed write can still leave data behind, hence why it's in the async block
                tokio::fs::write(&limbo_path, entry.source.as_bytes())
                    .await
                    .map_err(|err| anyhow!("Failed to write to file, Path: {}, Error: {}", limbo_path.display(), err))?;
                utils::chown(&limbo_path, entry.uid, entry.gid).await?;
                utils::chmod(&limbo_path, entry.mode).await
            }
            .await
            {
                match tokio::fs::remove_file(&limbo_path).await {
                    Ok(()) => return Err(err),
                    Err(remove_err) => return Err(anyhow!("{}, Double whammy, file potentially stuck in limbo {}: {}", err, limbo_path.display(), remove_err)),
                }
            }

            let bak = utils::atomic_install(file_path, &limbo_path).await?;
            created_paths.disarm();
            print_suc(format!("Wrote to, Path: {}", file_path.display()).as_str()).await;
            utils::handle_clobber(bak).await?;
            Ok(())
        });
    }

    while let Some(result) = set.join_next().await {
        result??;
    }

    Ok(())
}
