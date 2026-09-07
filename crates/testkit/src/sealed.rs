use std::{io, path::Path, process::Command};

/// Builds, but never starts, a child isolated from ambient user configuration.
/// Both paths must be absolute. All created directories stay under `temp_root`.
/// Cargo build commands need a separate explicit environment, not this helper.
pub fn sealed_command(binary: &Path, temp_root: &Path) -> io::Result<Command> {
    if !binary.is_absolute() || !temp_root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "executable and temporary root must be absolute",
        ));
    }
    let home = temp_root.join("home");
    let config = temp_root.join("config");
    let cache = temp_root.join("cache");
    let data = temp_root.join("data");
    let state = temp_root.join("state");
    let tmp = temp_root.join("tmp");
    for path in [&home, &config, &cache, &data, &state, &tmp] {
        std::fs::create_dir_all(path)?;
    }
    let mut command = Command::new(binary);
    command
        .env_clear()
        .current_dir(temp_root)
        .env("PATH", "")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_STATE_HOME", &state)
        .env("TMPDIR", &tmp)
        .env("TMP", &tmp)
        .env("TEMP", &tmp);
    #[cfg(windows)]
    {
        // Windows process loading requires the OS directory, not user settings.
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        command
            .env("USERPROFILE", &home)
            .env("APPDATA", &config)
            .env("LOCALAPPDATA", &data);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsString};

    #[test]
    fn constructs_without_executing_and_replaces_ambient_configuration() {
        let root = tempfile::tempdir().unwrap();
        // This executable does not exist; constructing must not attempt to run it.
        let binary = root.path().join("absent-executable");
        let command = sealed_command(&binary, root.path()).unwrap();
        assert_eq!(command.get_program(), binary.as_os_str());
        assert_eq!(command.get_current_dir(), Some(root.path()));
        let vars: BTreeMap<OsString, OsString> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.unwrap().to_owned()))
            .collect();
        assert_eq!(vars.get(&OsString::from("PATH")), Some(&OsString::from("")));
        assert_eq!(
            vars.get(&OsString::from("HOME")),
            Some(&root.path().join("home").into_os_string())
        );
        assert_eq!(
            vars.get(&OsString::from("XDG_CONFIG_HOME")),
            Some(&root.path().join("config").into_os_string())
        );
        assert!(!vars.contains_key(&OsString::from("UNISPHERE_CONFIG")));
        for leaf in ["home", "config", "cache", "data", "state", "tmp"] {
            assert!(root.path().join(leaf).is_dir());
        }
    }

    #[test]
    fn rejects_relative_paths_before_creating_directories() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("must-not-exist");
        assert_eq!(
            sealed_command(Path::new("relative"), &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!target.exists());
        assert_eq!(
            sealed_command(&root.path().join("binary"), Path::new("relative"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
