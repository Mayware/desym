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

pub async fn get_metadata(path: &Path) -> Result<Option<Metadata>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[derive(Default)]
pub struct CreatedPaths {
    paths: Vec<PathBuf>,
    armed: bool,
}

// Automatically rollback created paths, if the bomb wasn't disarmed
impl CreatedPaths {
    pub fn disarm(mut self) {
        self.armed = false;
    }
}
impl Drop for CreatedPaths {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        for path in self.paths.iter().rev() {
            let _ = std::fs::remove_dir(path);
        }
    }
}

pub async fn create_path(path: &Path, uid: u32, gid: u32) -> Result<CreatedPaths> {
    let mut created = CreatedPaths { paths: Vec::new(), armed: true };
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);

        if current.exists() {
            continue;
        }

        match tokio::fs::DirBuilder::new()
            // .mode(mode | 0o110)
            .create(&current)
            .await
        {
            Ok(()) => {
                created.paths.push(current.clone());
                chown(&current, uid, gid).await?;
                print_suc(format!("Created path: {}", current.display()).as_str()).await;
            }

            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(anyhow!(format!("Failed, could not create path: {}, Err: {}", current.display(), err)));
            }
        }
    }

    Ok(created)
}

pub async fn remove_path(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::IsADirectory => tokio::fs::remove_dir_all(path).await.map_err(Into::into),
        Err(err) => Err(err.into()),
    }
}

pub async fn require_parent_paths(path: &Path, uid: u32, gid: u32) -> Result<CreatedPaths> {
    match path.parent() {
        Some(parent) => create_path(parent, uid, gid).await,
        None => Ok(CreatedPaths::default()),
    }
}

pub fn get_bak<S: std::fmt::Display>(original: S) -> String {
    return format!("{}_{}.bak", original, uuid::Uuid::new_v4());
}

pub fn get_limbo<S: std::fmt::Display>(original: S) -> String {
    return format!("{}_{}.limbo", original, uuid::Uuid::new_v4());
}

pub async fn atomic_install(original: &Path, limbo: &Path) -> Result<Option<String>> {
    let is_overwriting = match tokio::fs::symlink_metadata(original).await {
        Ok(_) => true,
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };

    if !is_overwriting {
        if let Err(err) = tokio::fs::rename(limbo, original).await {
            let _ = remove_path(limbo).await;
            return Err(err.into());
        }
        return Ok(None);
    }

    // Rustix's renameat doesn't take the syscall flags, renameat_with does. Exchange means what it
    // says on the tin, to swap the paths if both exist. ABS because limbo and original must be
    // absolute paths
    if let Err(err) = rustix::fs::renameat_with(rustix::fs::ABS, limbo, rustix::fs::ABS, original, rustix::fs::RenameFlags::EXCHANGE) {
        let _ = remove_path(limbo).await;
        return Err(err.into());
    }

    let bak = get_bak(original.to_string_lossy());
    tokio::fs::rename(limbo, &bak).await?; // Limbo now refers to the original file
    Ok(Some(bak))
}

pub async fn handle_clobber(bak: Option<String>) -> Result<()> {
    // Remains none, if there wasn't a backup to make (ie. we didn't clobber)
    if let Some(bak) = bak {
        let keep_clobbered = match settings().keep_clobbered {
            Some(keep_clobbered) => keep_clobbered,
            None => get_confirmation(&format!("Would you like to keep the clobbered file: {}?", bak)).await,
        };

        if !keep_clobbered {
            remove_path(Path::new(&bak)).await?;
        }
    }
    Ok(())
}

async fn print(prefix: &str, message: &str) {
    input().println(format!("{} {}", prefix, message).as_str()).await;
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
    let response = input().prompt(format!("{} {} [Y/n] ", "[CONFIRM]".bright_purple().to_string().as_str(), message).as_str()).await;
    input().println("").await;

    match response.trim().to_lowercase().as_str() {
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

pub async fn chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    if let Err(err) = lchown(path, Some(uid), Some(gid)) {
        return Err(anyhow!(format!("Failed to chown {} as {} {}: {}", path.display(), uid, gid, err,)));
    }
    Ok(())
}

pub async fn chmod(path: &Path, mode: u32) -> Result<()> {
    if let Err(err) = set_permissions(path, Permissions::from_mode(mode)).await {
        return Err(anyhow!(format!("Failed to chmod {} as {}: {}", path.display(), mode, err)));
    }
    Ok(())
}
