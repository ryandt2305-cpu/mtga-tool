// MTGA install discovery — locates the game's install root.
//
// Checks, in order:
//   1. The running MTGA.exe process (works for any launcher or custom path).
//   2. Known static install paths (Steam, Epic, Wizards standalone).
//   3. Steam library folders parsed from steamapps\libraryfolders.vdf.
//
// Divergence from the Python reference (python/src/card_database.py): the
// reference only checks the two static Steam/Epic defaults. Discovery via
// the running process, the Wizards standalone paths, and Steam library
// folders is Rust-only — added after a user with a non-default install hit
// CDB-001 with no recovery path.

use std::path::{Path, PathBuf};

use crate::services::memory::process;

/// Known static MTGA install paths (Steam default, Epic default,
/// Wizards standalone installer).
const STATIC_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Steam\steamapps\common\MTGA",
    r"C:\Program Files\Epic Games\MagicTheGathering",
    r"C:\Program Files\Wizards of the Coast\MTGA",
    r"C:\Program Files (x86)\Wizards of the Coast\MTGA",
];

/// Steam roots to look for steamapps\libraryfolders.vdf under.
const STEAM_ROOTS: &[&str] = &[r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"];

/// Locate the MTGA install directory.
///
/// On failure returns the list of locations checked, for the CDB-001
/// diagnostic message.
pub fn discover() -> Result<PathBuf, Vec<String>> {
    let mut checked: Vec<String> = Vec::new();

    // Source 1: the running MTGA.exe process — install root is the exe's
    // parent directory (Unity layout: <root>\MTGA.exe + <root>\MTGA_Data\).
    match install_dir_from_process() {
        Ok(dir) => {
            if is_mtga_install_dir(&dir) {
                return Ok(dir);
            }
            checked.push(format!("running MTGA.exe at {}", dir.display()));
        }
        Err(why) => checked.push(format!("running MTGA.exe ({})", why)),
    }

    // Source 2: static paths.
    for path_str in STATIC_PATHS {
        let path = Path::new(path_str);
        if is_mtga_install_dir(path) {
            return Ok(path.to_path_buf());
        }
        checked.push((*path_str).to_string());
    }

    // Source 3: Steam library folders on other drives.
    for root in STEAM_ROOTS {
        let vdf = Path::new(root).join("steamapps").join("libraryfolders.vdf");
        let Ok(contents) = std::fs::read_to_string(&vdf) else {
            continue; // no Steam here — the static default already covered it
        };
        for lib in parse_library_folders(&contents) {
            let candidate = lib.join("steamapps").join("common").join("MTGA");
            if is_mtga_install_dir(&candidate) {
                return Ok(candidate);
            }
            checked.push(candidate.display().to_string());
        }
    }

    Err(checked)
}

/// Derive the install root from the running MTGA.exe process.
fn install_dir_from_process() -> Result<PathBuf, String> {
    let pid = process::find_process("MTGA.exe")?.ok_or_else(|| "not running".to_string())?;
    let exe = process::process_exe_path(pid)?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("no parent dir for {}", exe.display()))
}

/// A directory is an MTGA install root if it contains MTGA_Data\
/// (where Raw_CardDatabase_*.mtga lives).
fn is_mtga_install_dir(dir: &Path) -> bool {
    dir.join("MTGA_Data").is_dir()
}

/// Extract library paths from Steam's libraryfolders.vdf contents.
///
/// Matches every `"path" "<value>"` pair and unescapes the doubled
/// backslashes VDF uses in Windows paths.
fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    let re = regex::Regex::new(r#""path"\s+"([^"]*)""#).expect("static regex is valid");
    re.captures_iter(vdf)
        .map(|c| PathBuf::from(c[1].replace(r"\\", r"\")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_library_folders_extracts_all_paths() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        assert_eq!(
            parse_library_folders(vdf),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn parse_library_folders_ignores_other_keys_and_malformed_input() {
        assert_eq!(parse_library_folders(""), Vec::<PathBuf>::new());
        assert_eq!(
            parse_library_folders(r#""apps" { "2141910" "1234" } "path" no-quotes"#),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn is_mtga_install_dir_requires_mtga_data() {
        let base = std::env::temp_dir().join(format!("mtga_install_test_{}", std::process::id()));
        let install = base.join("MTGA");
        std::fs::create_dir_all(install.join("MTGA_Data")).expect("create test dirs");
        let empty = base.join("NotMTGA");
        std::fs::create_dir_all(&empty).expect("create test dirs");

        assert!(is_mtga_install_dir(&install));
        assert!(!is_mtga_install_dir(&empty));
        assert!(!is_mtga_install_dir(&base.join("missing")));

        let _ = std::fs::remove_dir_all(&base);
    }
}
