use std::{
    fs::{Metadata, Permissions},
    os::unix::fs::{PermissionsExt, lchown},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Result, anyhow};

use owo_colors::OwoColorize;
use tokio::{fs::set_permissions, io};

use crate::{Settings, input::Input};

static SETTINGS: OnceLock<Settings> = OnceLock::new();
static INPUT: OnceLock<Input> = OnceLock::new();

pub fn init_settings(settings: Settings) {
    let _ = SETTINGS.set(settings);
}

pub fn settings() -> &'static Settings {
    SETTINGS.get().expect("settings not initialized")
}

pub fn input() -> &'static Input {
    INPUT.get_or_init(Input::new)
}

// Returns the Some(Metadata) if the file exists, returns None if the file didn't exist, and errors
// upon any other error. The caller then can put specific logic for if the file exists, and to
// verify it is still valid
pub async fn get_matching_metadata(path: &Path, uid: u32, gid: u32) -> Result<Option<Metadata>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // Check if parent directories also need creation
            let needs_parent = path.parent().map(|p| !p.exists()).unwrap_or(false);
            if needs_parent {
                create_path(path.parent().unwrap(), uid, gid).await?;
            }
            Ok(None)
        }
        Err(err) => {
            return Err(anyhow!(format!(
                "Failed, could not stat path: {}, Err: {}",
                path.display(),
                err
            )));
        }
    }
}

pub async fn create_path(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let confirmation = !settings().add_path_confirmation
        || get_confirmation(
            format!(
                "The path {}, does not exist. Would you to create it?",
                path.display(),
            )
            .as_str(),
        )
        .await;

    if !confirmation {
        return Err(anyhow!(format!(
            "Aborting, could not create path: {}",
            path.display()
        )));
    }

    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);

        if current.exists() {
            continue;
        }

        match tokio::fs::DirBuilder::new()
            // Always have the executable bits for owner/group on the directory, so it is
            // traversable. Although hardcoding it isn't ideal, we can't infer from the actual
            // target mode. Update - just don't inherit the mode
            // .mode(mode | 0o110)
            .create(&current)
            .await
        {
            Ok(()) => {
                raw_chown(&current, uid, gid).await?;
                print_suc(format!("Created path: {}", current.display()).as_str()).await;
            }

            Err(err) => {
                return Err(anyhow!(format!(
                    "Failed, could not create path: {}, Err: {}",
                    current.display(),
                    err
                )));
            }
        }
    }

    Ok(())
}

pub async fn remove_path(path: &Path) -> Result<()> {
    let confirmation = !settings().remove_path_confirmation
        || get_confirmation(
            format!(
                "The path {}, exists. Would you like to remove it?",
                path.display(),
            )
            .as_str(),
        )
        .await;

    if confirmation {
        let result = match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::IsADirectory => std::fs::remove_dir_all(path),
            Err(err) => Err(err),
        };

        match result {
            Ok(()) => {
                print_suc(format!("Removed path: {}", path.display()).as_str()).await;
                return Ok(());
            }
            Err(err) => {
                return Err(anyhow!(format!(
                    "Failed, could not remove path: {}, Err: {}",
                    path.display(),
                    err
                )));
            }
        }
    } else {
        return Err(anyhow!(format!(
            "Aborting, could not remove path: {}",
            path.display()
        )));
    }
}

async fn print(prefix: &str, message: &str) {
    input()
        .println(format!("{} {}", prefix, message).as_str())
        .await;
}

pub async fn print_err(message: &str) {
    print("[ERROR]".red().to_string().as_str(), message).await;
}

pub async fn print_real_err(err: &anyhow::Error) {
    print_err(err.to_string().as_str()).await;
    for cause in err.chain().skip(1) {
        print("[CAUSE]".red().to_string().as_str(), &cause.to_string()).await;
    }
}

pub async fn print_suc(message: &str) {
    print("[SUCCESS]".green().to_string().as_str(), message).await;
}

pub async fn get_confirmation(message: &str) -> bool {
    let input = input()
        .prompt(
            format!(
                "{} {} [Y/n] ",
                "[CONFIRM]".bright_purple().to_string().as_str(),
                message
            )
            .as_str(),
        )
        .await;

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" | "" => {
            return true;
        }
        "n" | "no" => {
            return false;
        }
        _ => {
            print_err("Input was invalid, assuming no!").await;
            return false;
        }
    }
}

pub async fn raw_chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    if let Err(err) = lchown(path, Some(uid), Some(gid)) {
        return Err(anyhow!(format!(
            "Failed to chown {} as {} {}: {}",
            path.display(),
            uid,
            gid,
            err,
        )));
    }
    Ok(())
}

pub async fn raw_chmod(path: &Path, mode: u32) -> Result<()> {
    if let Err(err) = set_permissions(path, Permissions::from_mode(mode)).await {
        return Err(anyhow!(format!(
            "Failed to chmod {} as {}: {}",
            path.display(),
            mode,
            err
        )));
    }
    Ok(())
}
