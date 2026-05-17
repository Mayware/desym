use std::collections::HashMap;
use std::os::unix;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tokio::task::JoinSet;

use crate::utils::print_suc;
use crate::{Entry, utils};
use anyhow::{Result, anyhow};

async fn create_symlink(symlink_path: &Path, entry: Entry) -> Result<()> {
    match unix::fs::symlink(entry.source.clone(), symlink_path) {
        Ok(()) => {
            // Apparently the symlink file permissions don't matter, on unixes apart from MacOS
            // but can't hurt to be consistent
            // It cries on linux, so i've vaulted it
            // utils::raw_chown(symlink_path, entry.uid, entry.gid).await?;
            // utils::raw_chmod(symlink_path, entry.mode).await?;

            print_suc(
                format!(
                    "Created symlink: {} -> {}",
                    symlink_path.display(),
                    entry.source
                )
                .as_str(),
            )
            .await;

            Ok(())
        }
        Err(err) => {
            return Err(anyhow!(format!(
                "Failed, could not create symlink: {} -> {}, Err: {}",
                symlink_path.display(),
                entry.source,
                err
            )));
        }
    }
}

pub async fn process(symlinks: HashMap<String, Entry>) -> Result<()> {
    let mut set: JoinSet<Result<()>> = JoinSet::new();

    for (symlink_path, entry) in symlinks {
        set.spawn(async move {
            let symlink_path = Path::new(&symlink_path);
            let base_path = Path::new(&entry.source);

            if let Some(metadata) =
                utils::get_matching_metadata(&symlink_path, entry.uid, entry.gid, entry.mode)
                    .await?
            {
                if metadata.is_symlink() {
                    let link_target = tokio::fs::read_link(symlink_path).await?;

                    // Expand, if it was relative. Although, relative links should never really
                    // be done through us, since we don't handle it consistently (for example,
                    // if the programs CWD changes, we'd be pointing at a different directory at
                    // creation time
                    let resolved_target = if link_target.is_absolute() {
                        link_target
                    } else {
                        symlink_path
                            .parent()
                            .unwrap_or(Path::new(""))
                            .join(&link_target)
                    };

                    // No-op if everything matches
                    if resolved_target == base_path
                        && metadata.uid() == entry.uid
                        && metadata.gid() == entry.gid
                        && metadata.mode() == entry.mode
                    {
                        return Ok(());
                    }
                }
                utils::remove_path(symlink_path).await?;
            }

            create_symlink(symlink_path, entry).await?;
            Ok(())
        });
    }

    while let Some(result) = set.join_next().await {
        result??;
    }

    Ok(())
}
