//! Downloadable Hunspell dictionaries from a pinned LibreOffice revision.
//!
//! Packages are intentionally a closed catalogue.  Settings never accept a
//! user-supplied URL: each downloaded file must match the SHA-256 recorded
//! here before it is installed under `data/dictionaries`.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const LIBREOFFICE_COMMIT: &str = "32b006a2c22a4ac7e8ed3f03346f7b3d85a970a4";

/// A dictionary package the application knows how to download safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    pub id: &'static str,
    pub language: &'static str,
    pub label: &'static str,
    pub license: &'static str,
    aff_path: &'static str,
    aff_sha256: &'static str,
    dic_path: &'static str,
    dic_sha256: &'static str,
}

/// Optional British English spelling.  The built-in RU and US English
/// dictionaries remain available offline and do not need downloading.
pub const EN_GB: Package = Package {
    id: "en-gb",
    language: "en",
    label: "English (United Kingdom)",
    license: "SCOWL / permissive licenses; see LibreOffice dictionary notice",
    aff_path: "en/en_GB.aff",
    aff_sha256: "0fd6ed120ef28957847d98ba5149b117e27116cf81b5aa36208453f6755a36fd",
    dic_path: "en/en_GB.dic",
    dic_sha256: "04e90f34f5263bf26780e9c4a442e9ad16584e227af49ddd1b3b21b01df5b29c",
};

pub const PACKAGES: &[Package] = &[EN_GB];

/// Returns the package selected by its persisted identifier.
pub fn package(id: &str) -> Option<Package> {
    PACKAGES.iter().copied().find(|item| item.id == id)
}

/// The completed package location below `data/dictionaries`.
pub fn package_dir(root: &Path, package: Package) -> PathBuf {
    root.join(package.id)
}

/// Downloads, hashes and atomically installs a package.
pub fn install(root: &Path, package: Package) -> Result<()> {
    let directory = package_dir(root, package);
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;
    fetch_checked(
        package.aff_path,
        package.aff_sha256,
        &directory.join("dictionary.aff"),
    )?;
    fetch_checked(
        package.dic_path,
        package.dic_sha256,
        &directory.join("dictionary.dic"),
    )?;
    Ok(())
}

fn fetch_checked(source: &str, expected_hash: &str, target: &Path) -> Result<()> {
    let url = format!(
        "https://raw.githubusercontent.com/LibreOffice/dictionaries/{LIBREOFFICE_COMMIT}/{source}"
    );
    let mut response = ureq::get(&url)
        .call()
        .with_context(|| format!("cannot download dictionary file {source}"))?;
    let data = response
        .body_mut()
        .read_to_vec()
        .with_context(|| format!("cannot read dictionary file {source}"))?;
    let actual = format!("{:x}", Sha256::digest(&data));
    if actual != expected_hash {
        bail!("dictionary file {source} did not match its published SHA-256");
    }
    if !data.starts_with(b"SET ") && source.ends_with(".aff") {
        bail!("dictionary affix file {source} is not a Hunspell UTF-8 file");
    }
    if data.is_empty() || source.ends_with(".dic") && !data[0].is_ascii_digit() {
        bail!("dictionary word list {source} is not a Hunspell dictionary");
    }
    let parent = target.parent().context("dictionary target has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&data)?;
    temporary.persist(target).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_unique_safe_package_locations() {
        assert!(package("en-gb").is_some());
        assert!(package("https://example.invalid").is_none());
        let root = tempfile::tempdir().unwrap();
        assert_eq!(package_dir(root.path(), EN_GB), root.path().join("en-gb"));
    }
}
