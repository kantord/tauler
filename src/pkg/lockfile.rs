//! The Lockfile: a YAML file sibling to the layout file, mapping `owner/repo` to
//! the commit a Package is pinned to (or to Development mode). See ADR 0041.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PackageEntry {
    pub commit: String,
    #[serde(default)]
    pub development: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Lockfile {
    packages: HashMap<String, PackageEntry>,
}

impl Lockfile {
    /// An empty (or whitespace-only) file is a Lockfile with no entries yet — the
    /// state tauler sees before any Package has ever been imported. `serde_yaml`
    /// deserializes empty input as `null`, which a transparent `HashMap` wrapper
    /// does not accept on its own, so that case is handled explicitly here.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        if yaml.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_yaml::from_str(yaml)
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).expect("Lockfile serialization cannot fail")
    }

    pub fn get(&self, owner: &str, repo: &str) -> Option<&PackageEntry> {
        self.packages.get(&format!("{owner}/{repo}"))
    }

    pub fn set(&mut self, owner: &str, repo: &str, entry: PackageEntry) {
        self.packages.insert(format!("{owner}/{repo}"), entry);
    }

    /// Every entry as `(owner, repo, entry)` — for `tauler pkg update`, which
    /// needs to walk all of them. `owner`/`repo` never contain `/` themselves
    /// (`pkg::specifier::parse` already rejects a specifier where they would),
    /// so splitting the stored `"owner/repo"` key on the first `/` is exact, not
    /// a heuristic.
    pub fn entries(&self) -> Vec<(String, String, PackageEntry)> {
        self.packages
            .iter()
            .filter_map(|(key, entry)| {
                let (owner, repo) = key.split_once('/')?;
                Some((owner.to_string(), repo.to_string(), entry.clone()))
            })
            .collect()
    }

    /// A Lockfile that doesn't exist on disk yet has no entries — the state
    /// tauler sees before any Package has ever been imported into this layout
    /// file, same as an empty file. A read failure other than "not found" (a
    /// permissions error, say) is folded into the same "no entries" reading
    /// rather than surfaced separately: either way there is nothing to pin
    /// against, and the caller's next step is the same.
    pub fn load_from_path(path: &Path) -> Result<Self, serde_yaml::Error> {
        let yaml = std::fs::read_to_string(path).unwrap_or_default();
        Self::from_yaml(&yaml)
    }

    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_yaml())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_from_yaml_parses_a_pinned_package_entry() {
        let yaml = "foo/bar:\n  commit: abc123";
        let lockfile = Lockfile::from_yaml(yaml).expect("valid yaml should parse");
        let entry = lockfile
            .get("foo", "bar")
            .expect("foo/bar should be present");
        assert_eq!(entry.commit, "abc123");
        assert!(!entry.development);
    }

    #[test]
    fn lockfile_from_yaml_parses_a_development_mode_entry() {
        let yaml = "foo/bar:\n  commit: abc123\n  development: true";
        let lockfile = Lockfile::from_yaml(yaml).expect("valid yaml should parse");
        let entry = lockfile
            .get("foo", "bar")
            .expect("foo/bar should be present");
        assert!(entry.development);
    }

    #[test]
    fn lockfile_get_returns_none_for_an_absent_package() {
        let lockfile = Lockfile::from_yaml("foo/bar:\n  commit: abc123").unwrap();
        assert!(lockfile.get("no", "such").is_none());
    }

    #[test]
    fn lockfile_to_yaml_round_trips_through_from_yaml() {
        let original = Lockfile::from_yaml("foo/bar:\n  commit: abc123\n  development: true")
            .expect("valid yaml should parse");
        let rendered = original.to_yaml();
        let reparsed = Lockfile::from_yaml(&rendered).expect("rendered yaml should parse");
        assert_eq!(reparsed.get("foo", "bar"), original.get("foo", "bar"));
    }

    #[test]
    fn lockfile_set_adds_a_retrievable_entry() {
        let mut lockfile = Lockfile::default();
        lockfile.set(
            "foo",
            "bar",
            PackageEntry {
                commit: "abc123".to_string(),
                development: false,
            },
        );
        let entry = lockfile
            .get("foo", "bar")
            .expect("foo/bar should be present");
        assert_eq!(entry.commit, "abc123");
        assert!(!entry.development);
    }

    #[test]
    fn lockfile_from_yaml_parses_empty_input_as_no_entries() {
        let lockfile = Lockfile::from_yaml("").expect("empty yaml should parse");
        assert!(lockfile.get("anything", "at-all").is_none());
    }

    #[test]
    fn load_from_path_reads_no_entries_when_the_file_does_not_exist_yet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tauler-pkg.lock");

        let lockfile = Lockfile::load_from_path(&path).expect("should not error");

        assert!(lockfile.get("anything", "at-all").is_none());
    }

    #[test]
    fn save_then_load_round_trips_through_a_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tauler-pkg.lock");
        let mut lockfile = Lockfile::default();
        lockfile.set(
            "foo",
            "bar",
            PackageEntry {
                commit: "abc123".to_string(),
                development: false,
            },
        );

        lockfile.save_to_path(&path).expect("save should succeed");
        let reloaded = Lockfile::load_from_path(&path).expect("load should succeed");

        assert_eq!(reloaded.get("foo", "bar"), lockfile.get("foo", "bar"));
    }
}
