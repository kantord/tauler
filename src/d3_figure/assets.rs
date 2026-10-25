//! The two third-party files a D3Figure needs — d3 and linkedom — and the on-disk
//! cache that holds them. ADR 0045 says why they are downloaded rather than
//! vendored, and why each pin points where it does.
//!
//! This unit does no networking: [`store`] takes bytes someone else downloaded,
//! [`load`] reads what [`store`] wrote. Both check the file against the sha256
//! pinned in its [`Asset`], and `load` does so on *every* call, not only after a
//! download — a file changed in the cache is refused exactly like a bad download,
//! so the caller fetches it again.
//!
//! The pin is the hash of the file itself, not of what was downloaded: d3 comes
//! as an npm `.tgz` whose bytes (compression, member order, timestamps) are npm's
//! business, while `d3.min.js` inside it is d3's. Pinning the member means a
//! repacked tarball with the same file inside still verifies.
//!
//! `store` writes through a temp file in the cache dir and renames it into place,
//! so a reader never sees a half-written file and a failed store leaves nothing
//! behind — the temp file is removed when it is dropped without being persisted.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

/// One third-party file: where it is published, which member to take if the
/// download is a .tgz, and the sha256 of the file itself (not of the archive).
pub struct Asset {
    /// Name in the cache dir, e.g. `"d3.min.js"`.
    pub file_name: &'static str,
    pub url: &'static str,
    /// `Some("package/dist/d3.min.js")` for an npm .tgz, `None` for a raw file.
    pub tar_member: Option<&'static str>,
    /// Lowercase hex.
    pub sha256: &'static str,
}

/// d3 7.9.0's built bundle, from npm's tarball — d3 publishes built files nowhere
/// else that is pinned to a version.
pub const D3: Asset = Asset {
    file_name: "d3.min.js",
    url: "https://registry.npmjs.org/d3/-/d3-7.9.0.tgz",
    tar_member: Some("package/dist/d3.min.js"),
    sha256: "f2094bbf6141b359722c4fe454eb6c4b0f0e42cc10cc7af921fc158fceb86539",
};

/// linkedom 0.18.14's single-file build, from GitHub at the tag's commit (a tag
/// can move; a commit cannot).
pub const LINKEDOM: Asset = Asset {
    file_name: "linkedom.js",
    url: "https://raw.githubusercontent.com/WebReflection/linkedom/e2ee20813e4b3ae946d7ba41621ddf3698431989/worker.js",
    tar_member: None,
    sha256: "1fac11c0c5d36109adef553fb2b03afe2c790719133d9e8f8ff59b5e28511148",
};

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("{file}: sha256 is {actual}, pinned {expected}")]
    HashMismatch {
        file: &'static str,
        expected: &'static str,
        actual: String,
    },
    #[error("{file}: the archive has no member {member}")]
    MemberMissing {
        file: &'static str,
        member: &'static str,
    },
    #[error("{file}: the archive member {member} is larger than {max} bytes")]
    MemberTooLarge {
        file: &'static str,
        member: &'static str,
        max: u64,
    },
    #[error("{file}: not a readable .tgz: {source}")]
    Archive {
        file: &'static str,
        source: std::io::Error,
    },
    #[error("{file}: {source}")]
    Io {
        file: &'static str,
        source: std::io::Error,
    },
}

/// The cached file's contents, only if it exists and its sha256 matches the pin.
pub fn load(asset: &Asset, dir: &Path) -> Option<String> {
    let bytes = std::fs::read(dir.join(asset.file_name)).ok()?;
    let actual = sha256_hex(&bytes);
    if actual != asset.sha256 {
        tracing::warn!(
            file = asset.file_name,
            actual,
            expected = asset.sha256,
            "cached file does not match its pinned sha256; ignoring it"
        );
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Take a downloaded body (the .tgz or the raw file), extract the member if any
/// (at most [`MAX_MEMBER_BYTES`]), verify the sha256, and write it to `dir/file_name` atomically (temp file in
/// `dir`, then rename). Nothing is written when anything fails.
pub fn store(asset: &Asset, dir: &Path, downloaded: &[u8]) -> Result<(), AssetError> {
    let file = asset.file_name;
    let body = match asset.tar_member {
        Some(member) => extract_member(file, member, downloaded, MAX_MEMBER_BYTES)?,
        None => downloaded.to_vec(),
    };
    let actual = sha256_hex(&body);
    if actual != asset.sha256 {
        return Err(AssetError::HashMismatch {
            file,
            expected: asset.sha256,
            actual,
        });
    }

    let io = |source| AssetError::Io { file, source };
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
    temp.write_all(&body).map_err(io)?;
    temp.persist(dir.join(file)).map_err(|e| io(e.error))?;
    Ok(())
}

/// The most an extracted archive member may hold. d3.min.js is ~280 KB, so
/// this only stops a runaway archive.
const MAX_MEMBER_BYTES: u64 = 16 * 1024 * 1024;

/// The bytes of `member` inside the gzipped tarball `tgz`, refused if there
/// are more than `max` of them.
fn extract_member(
    file: &'static str,
    member: &'static str,
    tgz: &[u8],
    max: u64,
) -> Result<Vec<u8>, AssetError> {
    let archive_err = |source| AssetError::Archive { file, source };
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tgz));
    for entry in archive.entries().map_err(archive_err)? {
        let entry = entry.map_err(archive_err)?;
        if entry.path().map_err(archive_err)? != Path::new(member) {
            continue;
        }
        // Read one byte past `max` rather than trusting the header's size.
        let mut bytes = Vec::new();
        entry
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(archive_err)?;
        if bytes.len() as u64 > max {
            return Err(AssetError::MemberTooLarge { file, member, max });
        }
        return Ok(bytes);
    }
    Err(AssetError::MemberMissing { file, member })
}

/// The lowercase hex sha256 of `bytes`, as [`Asset::sha256`] pins it.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// An [`Asset`] pinned to `content`, for tests: nothing but the real files
/// matches the real pins. Leaked because `Asset` holds `'static` strings — it
/// describes compile-time constants; a test builds a handful.
#[cfg(test)]
pub(crate) fn pinned_to(
    file_name: &'static str,
    url: &'static str,
    tar_member: Option<&'static str>,
    content: &str,
) -> &'static Asset {
    Box::leak(Box::new(Asset {
        file_name,
        url,
        tar_member,
        sha256: Box::leak(sha256_hex(content.as_bytes()).into_boxed_str()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &str = "export const answer = 42;\n";
    const MEMBER: &str = "package/dist/thing.min.js";

    /// An [`Asset`] pinned to `CONTENT`.
    fn asset(tar_member: Option<&'static str>) -> &'static Asset {
        pinned_to(
            "thing.min.js",
            "https://example.invalid/thing",
            tar_member,
            CONTENT,
        )
    }

    /// A gzipped tarball holding `content` at `path`, as npm would serve it.
    fn tgz(path: &str, content: &[u8]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, content)
            .expect("append member");
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip")
    }

    fn entries(dir: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .expect("read cache dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect()
    }

    #[test]
    fn a_file_that_is_not_cached_loads_as_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(load(asset(None), dir.path()), None);
    }

    #[test]
    fn a_cached_file_whose_bytes_match_the_pin_loads_as_its_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(None);
        std::fs::write(dir.path().join(asset.file_name), CONTENT).expect("write cached file");

        assert_eq!(load(asset, dir.path()).as_deref(), Some(CONTENT));
    }

    #[test]
    fn a_cached_file_changed_after_download_does_not_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(None);
        store(asset, dir.path(), CONTENT.as_bytes()).expect("store");
        std::fs::write(
            dir.path().join(asset.file_name),
            "export const answer = 43;\n",
        )
        .expect("tamper with cached file");

        assert_eq!(load(asset, dir.path()), None);
    }

    #[test]
    fn storing_a_raw_download_that_matches_the_pin_makes_it_loadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(None);

        store(asset, dir.path(), CONTENT.as_bytes()).expect("store");

        assert_eq!(load(asset, dir.path()).as_deref(), Some(CONTENT));
    }

    #[test]
    fn storing_a_raw_download_that_does_not_match_the_pin_fails_and_leaves_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(None);

        let result = store(asset, dir.path(), b"something else entirely");

        assert!(
            matches!(result, Err(AssetError::HashMismatch { .. })),
            "{result:?}"
        );
        assert_eq!(entries(dir.path()), Vec::<std::path::PathBuf>::new());
    }

    #[test]
    fn storing_a_tgz_extracts_the_named_member_which_then_loads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(Some(MEMBER));

        store(asset, dir.path(), &tgz(MEMBER, CONTENT.as_bytes())).expect("store");

        assert_eq!(load(asset, dir.path()).as_deref(), Some(CONTENT));
    }

    #[test]
    fn storing_a_tgz_without_the_named_member_fails_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let asset = asset(Some(MEMBER));

        let result = store(
            asset,
            dir.path(),
            &tgz("package/README.md", CONTENT.as_bytes()),
        );

        assert!(
            matches!(result, Err(AssetError::MemberMissing { .. })),
            "{result:?}"
        );
        assert_eq!(entries(dir.path()), Vec::<std::path::PathBuf>::new());
    }

    #[test]
    fn a_tgz_member_larger_than_the_cap_is_refused() {
        let result = extract_member(
            "thing.min.js",
            MEMBER,
            &tgz(MEMBER, CONTENT.as_bytes()),
            CONTENT.len() as u64 - 1,
        );

        assert!(
            matches!(result, Err(AssetError::MemberTooLarge { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn a_tgz_member_exactly_at_the_cap_is_extracted() {
        let result = extract_member(
            "thing.min.js",
            MEMBER,
            &tgz(MEMBER, CONTENT.as_bytes()),
            CONTENT.len() as u64,
        );

        assert_eq!(result.ok(), Some(CONTENT.as_bytes().to_vec()));
    }

    /// Proves the real pins: downloads what `D3` and `LINKEDOM` point at and
    /// checks it verifies. Ignored so the suite never needs the network.
    #[test]
    #[ignore = "downloads from npm and GitHub"]
    fn the_real_d3_and_linkedom_downloads_match_their_pins() {
        let dir = tempfile::tempdir().expect("tempdir");
        for asset in [&D3, &LINKEDOM] {
            let body = super::super::fetch::download_https(asset.url).expect("download");
            store(asset, dir.path(), &body).expect("store");
            assert!(load(asset, dir.path()).is_some(), "{}", asset.file_name);
        }
    }
}
