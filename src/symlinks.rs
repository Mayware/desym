use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::task::JoinSet;

use crate::utils::print_suc;
use crate::{Entry, utils};
use anyhow::{Result, anyhow};

pub async fn process(symlinks: HashMap<String, Entry>) -> Result<()> {
    // Please note, symlink permissions don't matter - I initially implemented them
    // thinking linux would just ignore it, but no, it errors instead, so you'll see vaulted
    // associated code pieces, but entry is still taken for one glorious day in the future
    // where it may work.
    let mut set: JoinSet<Result<()>> = JoinSet::new();

    for (symlink_path, entry) in symlinks {
        set.spawn(async move {
            let symlink_path = Path::new(&symlink_path);
            let base_path = Path::new(&entry.source);

            let metadata = utils::get_metadata(symlink_path).await?;
            if let Some(metadata) = &metadata {
                if metadata.is_symlink() {
                    let link_target = tokio::fs::read_link(symlink_path).await?;
                    // No-op if everything matches
                    if link_target == base_path {
                        return Ok(());
                    }
                }
            }

            let created_paths = utils::require_parent_paths(symlink_path, entry.uid, entry.gid).await?;
            let limbo_path = PathBuf::from(utils::get_limbo(symlink_path.to_string_lossy()));
            match tokio::fs::symlink(entry.source.clone(), &limbo_path).await {
                Ok(()) => {
                    // Apparently the symlink file permissions don't matter, on unixes apart from MacOS
                    // and it didn't even work for me
                }
                Err(err) => return Err(anyhow!(format!("Failed, could not create symlink: {} -> {}, Err: {}", symlink_path.display(), entry.source, err))),
            }

            let bak = utils::atomic_install(symlink_path, &limbo_path).await?;
            created_paths.disarm();
            print_suc(format!("Created symlink: {} -> {}", symlink_path.display(), entry.source).as_str()).await;
            utils::handle_clobber(bak).await?;
            Ok(())
        });
    }

    while let Some(result) = set.join_next().await {
        result??;
    }

    Ok(())
}
