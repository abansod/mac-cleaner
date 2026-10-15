//! File-based discovery; no installed Podman executable is required.
use crate::safety::home_dir;
use anyhow::{bail, Result};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Roots {
    pub data: PathBuf,
    pub config: PathBuf,
}
impl Roots {
    pub fn detect() -> Self {
        let base = |key: &str, fallback: &str| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home_dir().join(fallback))
        };
        Self {
            data: base("XDG_DATA_HOME", ".local/share").join("containers/podman/machine"),
            config: base("XDG_CONFIG_HOME", ".config").join("containers/podman/machine"),
        }
    }
}

pub fn is_running() -> bool {
    // Fail closed on process-inspection errors; conservative across VM providers.
    Command::new("pgrep")
        .args([
            "-f",
            "vfkit|krunkit|qemu-system|gvproxy|[p]odman.*machine|[p]odman-desktop|[P]odman Desktop",
        ])
        .output()
        .map_or(true, |o| o.status.code() != Some(1))
}

fn no_symlinks(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        && path.ancestors().all(|p| {
            p.symlink_metadata()
                .is_ok_and(|m| !m.file_type().is_symlink())
        })
}

fn children(path: &Path) -> Result<Vec<PathBuf>> {
    match fs::read_dir(path) {
        Ok(entries) => entries
            .map(|e| e.map(|e| e.path()).map_err(Into::into))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A dangling configuration-directory link is not an absent directory.
            if path.symlink_metadata().is_ok() {
                bail!("Unusable Podman directory");
            }
            Ok(Vec::new())
        }
        Err(e) => Err(e.into()),
    }
}

fn configs(root: &Path, values: &mut Vec<Value>) -> Result<()> {
    if root.exists() && !no_symlinks(root) {
        bail!("Symlink in Podman configuration");
    }
    for path in children(root)? {
        if !no_symlinks(&path) {
            bail!("Symlink in Podman configuration");
        }
        if path.is_dir() {
            configs(&path, values)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
            // Unknown versions are not evidence that disks are unused.
            let image = value.get("ImagePath");
            if !image.is_some_and(|v| {
                v.as_str().is_some_and(|s| !s.is_empty())
                    || v.get("Path")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty())
            }) {
                bail!("Unknown Podman machine configuration");
            }
            values.push(value);
        }
    }
    Ok(())
}

fn references(value: &Value, path: &Path) -> bool {
    match value {
        Value::String(s) => {
            let p = Path::new(s);
            p == path
                || p.file_name() == path.file_name()
                || p.canonicalize()
                    .ok()
                    .zip(path.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b)
        }
        Value::Array(a) => a.iter().any(|v| references(v, path)),
        Value::Object(o) => o.values().any(|v| references(v, path)),
        _ => false,
    }
}

pub fn candidates(roots: &Roots, cache: bool) -> Result<Vec<PathBuf>> {
    let mut machines = Vec::new();
    configs(&roots.config, &mut machines)?;
    let mut found = Vec::new();
    for provider in children(&roots.data)? {
        if !no_symlinks(&provider) || !provider.is_dir() {
            continue;
        }
        if !provider
            .file_name()
            .is_some_and(|n| n == "applehv" || n == "qemu" || n == "libkrun")
        {
            continue;
        }
        let dir = if cache {
            provider.join("cache")
        } else {
            provider
        };
        for path in children(&dir)? {
            if !no_symlinks(&path) || !path.is_file() {
                continue;
            }
            if !cache && !path.extension().is_some_and(|e| e == "raw" || e == "qcow2") {
                continue;
            }
            if machines.iter().any(|v| references(v, &path)) {
                continue;
            }
            found.push(path);
        }
    }
    Ok(found)
}

pub fn check_reclaimable(path: &Path, cache: bool) -> Result<()> {
    if is_running() {
        bail!("Refused (stop Podman machines and quit Podman Desktop first)");
    }
    if !candidates(&Roots::detect(), cache)?
        .iter()
        .any(|p| p == path)
    {
        bail!("Refused (Podman may still use this path)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    struct Fixture {
        root: PathBuf,
        roots: Roots,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "mc-podman-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let roots = Roots {
                data: root.join("data"),
                config: root.join("config"),
            };
            fs::create_dir_all(roots.data.join("applehv/cache")).unwrap();
            fs::create_dir_all(roots.config.join("qemu")).unwrap();
            Self { root, roots }
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.roots.data.join(name);
            fs::write(&path, b"disk data").unwrap();
            path
        }
        fn config(&self, value: &str) {
            fs::write(self.roots.config.join("qemu/machine.json"), value).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn configured_disks_in_any_provider_are_protected_and_rechecked() {
        let f = Fixture::new();
        let active = f.file("applehv/active.raw");
        let orphan = f.file("applehv/old.qcow2");
        f.file("applehv/identity");
        f.config(
            &serde_json::json!({"ImagePath": {"Path": active}, "State": "stopped"}).to_string(),
        );
        assert_eq!(candidates(&f.roots, false).unwrap(), vec![orphan.clone()]);
        f.config(&serde_json::json!({"ImagePath": orphan}).to_string());
        assert!(!candidates(&f.roots, false).unwrap().contains(&orphan));
    }
    #[test]
    fn malformed_and_unknown_configs_fail_closed() {
        let f = Fixture::new();
        f.file("applehv/old.raw");
        for text in ["{", "{}", "null", "{\"ImagePath\":null}"] {
            f.config(text);
            assert!(candidates(&f.roots, false).is_err());
        }
    }
    #[test]
    fn caches_are_separate_and_symlinks_are_excluded() {
        let f = Fixture::new();
        let cache = f.file("applehv/cache/download.zst");
        let disk = f.file("applehv/old.raw");
        symlink(&disk, f.roots.data.join("applehv/link.raw")).unwrap();
        assert_eq!(candidates(&f.roots, true).unwrap(), vec![cache]);
        assert_eq!(candidates(&f.roots, false).unwrap(), vec![disk]);
        symlink(f.root.join("missing"), f.roots.config.join("qemu/bad.json")).unwrap();
        assert!(candidates(&f.roots, false).is_err());
    }
    #[test]
    fn symlinked_cache_and_configuration_directories_are_refused() {
        let f = Fixture::new();
        f.file("applehv/cache/download.zst");
        let config_alias = f.root.join("config-alias");
        symlink(&f.roots.config, &config_alias).unwrap();
        let aliases = Roots {
            data: f.roots.data.clone(),
            config: config_alias,
        };
        assert!(candidates(&aliases, false).is_err());
        fs::rename(f.roots.data.join("applehv/cache"), f.root.join("elsewhere")).unwrap();
        symlink(f.root.join("elsewhere"), f.roots.data.join("applehv/cache")).unwrap();
        assert!(candidates(&f.roots, true).unwrap().is_empty());
    }
    #[test]
    fn all_providers_are_scanned_but_unknown_storage_is_ignored() {
        let f = Fixture::new();
        for provider in ["qemu", "libkrun", "storage"] {
            fs::create_dir_all(f.roots.data.join(provider)).unwrap();
            f.file(&format!("{provider}/orphan.raw"));
        }
        assert_eq!(candidates(&f.roots, false).unwrap().len(), 2);
    }
    #[test]
    fn missing_installation_is_empty() {
        let f = Fixture::new();
        assert!(candidates(
            &Roots {
                data: f.root.join("absent-data"),
                config: f.root.join("absent-config")
            },
            false
        )
        .unwrap()
        .is_empty());
    }
}
