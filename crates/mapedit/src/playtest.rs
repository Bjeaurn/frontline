//! Playtest (F5, the menu, `mapedit/playtest`): the Map as it is being edited, in the game.
//!
//! MapEdit writes the working copy next to the Map's file (`maps/.mapedit-play-<id>.map`,
//! git-ignored, swept at the next start) and starts the game executable on it with
//! `--host --map-file <that file>`: the game's flag that loads a `.map` by path (see
//! `frontline::cli`). The executable is `frontline` next to the `mapedit` executable (what
//! `cargo build` leaves in `target/debug/`), or the file `MAPEDIT_GAME` names. The game runs as
//! a separate process: it reads the same `data/` from the working directory and, through
//! `BEVY_ASSET_ROOT`, the same assets as MapEdit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable that names the game executable.
pub const GAME_ENV: &str = "MAPEDIT_GAME";

/// The game executable: `MAPEDIT_GAME` when set, else `frontline` next to `own` (MapEdit's
/// own executable).
pub fn game_executable(env: Option<OsString>, own: &Path) -> Result<PathBuf, String> {
    if let Some(path) = env.filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!(
                "{GAME_ENV} is {}, which is not a file",
                path.display()
            ))
        };
    }
    let name = if cfg!(windows) {
        "frontline.exe"
    } else {
        "frontline"
    };
    let path = own
        .parent()
        .ok_or("MapEdit's own executable has no folder")?
        .join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "the game is not next to MapEdit ({}): build it (`cargo build`) or set {GAME_ENV} to its executable",
            path.display()
        ))
    }
}

/// The arguments that start a hosted game on a `.map` file.
pub fn arguments(map_file: &Path) -> Vec<OsString> {
    vec![
        "--host".into(),
        "--map-file".into(),
        map_file.as_os_str().to_owned(),
    ]
}

/// Starts the game on `map_file`, with `asset_root` (the folder that holds the game's `assets/`)
/// as its asset root. Returns the process id; a thread waits for the game to end and logs how.
pub fn launch(game: &Path, map_file: &Path, asset_root: &Path) -> Result<u32, String> {
    let mut child = Command::new(game)
        .args(arguments(map_file))
        .env("BEVY_ASSET_ROOT", asset_root)
        .spawn()
        .map_err(|err| format!("cannot start {}: {err}", game.display()))?;
    let pid = child.id();
    std::thread::spawn(move || match child.wait() {
        Ok(status) => bevy::log::info!("The playtest (pid {pid}) ended: {status}"),
        Err(err) => bevy::log::warn!("Cannot wait for the playtest (pid {pid}): {err}"),
    });
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mapedit-playtest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_game_is_found_next_to_mapedit_or_by_the_environment() {
        let dir = temp("find");
        let own = dir.join("mapedit");
        let game = dir.join(if cfg!(windows) {
            "frontline.exe"
        } else {
            "frontline"
        });
        let err = game_executable(None, &own).unwrap_err();
        assert!(
            err.contains("not next to MapEdit") && err.contains(GAME_ENV),
            "{err}"
        );
        std::fs::write(&game, "").unwrap();
        assert_eq!(game_executable(None, &own).unwrap(), game);
        // The variable wins, and must name a file.
        let other = dir.join("elsewhere");
        std::fs::write(&other, "").unwrap();
        assert_eq!(
            game_executable(Some(other.clone().into()), &own).unwrap(),
            other
        );
        assert!(game_executable(Some(dir.join("missing").into()), &own).is_err());
        assert_eq!(
            game_executable(Some(OsString::new()), &own).unwrap(),
            game,
            "an empty variable is no variable"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_game_starts_hosted_on_the_file() {
        let args = arguments(Path::new("/maps/.mapedit-play-x.map"));
        assert_eq!(args, ["--host", "--map-file", "/maps/.mapedit-play-x.map"]);
    }
}
