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
    license: "LGPL; see README_en_GB.txt",
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

/// Whether a fully downloaded, parseable package is present.
pub fn is_installed(root: &Path, package: Package) -> bool {
    load(root, package).is_ok()
}

pub fn load(root: &Path, package: Package) -> Result<spellbook::Dictionary> {
    let directory = package_dir(root, package);
    let aff = fs::read(directory.join("dictionary.aff"))?;
    let dic = fs::read(directory.join("dictionary.dic"))?;
    parse_checked(package, &aff, &dic)
}

fn checked_text<'a>(data: &'a [u8], expected: &str) -> Result<&'a str> {
    if format!("{:x}", Sha256::digest(data)) != expected {
        bail!("dictionary SHA-256 mismatch");
    }
    Ok(std::str::from_utf8(data)?.trim_start_matches('\u{feff}'))
}

fn parse_checked(package: Package, aff: &[u8], dic: &[u8]) -> Result<spellbook::Dictionary> {
    let aff = checked_text(aff, package.aff_sha256)?;
    let dic = checked_text(dic, package.dic_sha256)?;
    spellbook::Dictionary::new(aff, dic)
        .map_err(|err| anyhow::anyhow!("cannot parse Hunspell dictionary: {err}"))
}

/// Downloads, hashes and atomically installs a package.
pub fn install(root: &Path, package: Package) -> Result<()> {
    let aff = fetch(package.aff_path)?;
    let dic = fetch(package.dic_path)?;
    parse_checked(package, &aff, &dic)?;
    let notice = fetch("en/README_en_GB.txt")?;
    checked_text(
        &notice,
        "18b5833de60a52ffcb6e5b6b5048bb5a5d910acfecd5b096985b77058223880c",
    )?;
    // Validate the complete pair before touching installed files. Since this
    // catalogue is pinned, existing verified files are byte-for-byte identical.
    let directory = package_dir(root, package);
    fs::create_dir_all(&directory)?;
    for (name, data) in [
        ("README_en_GB.txt", &notice),
        ("dictionary.aff", &aff),
        ("dictionary.dic", &dic),
    ] {
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(data)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(directory.join(name))
            .map_err(|err| err.error)?;
    }
    Ok(())
}

fn fetch(source: &str) -> Result<Vec<u8>> {
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
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_and_comments_are_valid_but_corruption_is_not() {
        let aff = "\u{feff}# header\nSET UTF-8\n".as_bytes();
        let dic = "\u{feff}1\ncolour\n".as_bytes();
        let aff_hash = format!("{:x}", Sha256::digest(aff));
        let dic_hash = format!("{:x}", Sha256::digest(dic));
        let dictionary = spellbook::Dictionary::new(
            checked_text(aff, &aff_hash).unwrap(),
            checked_text(dic, &dic_hash).unwrap(),
        )
        .unwrap();
        assert!(dictionary.check("colour"));
        assert!(checked_text(b"corrupt", &dic_hash).is_err());
    }

    #[test]
    #[ignore = "downloads the pinned public dictionary into a temporary directory"]
    fn real_download_install_reload_and_repair() {
        let root = tempfile::tempdir().unwrap();
        assert!(!is_installed(root.path(), EN_GB));
        install(root.path(), EN_GB).unwrap();
        assert!(is_installed(root.path(), EN_GB));
        assert!(load(root.path(), EN_GB).unwrap().check("colour"));
        let file = package_dir(root.path(), EN_GB).join("dictionary.dic");
        fs::write(&file, "1\ncorrupt\n").unwrap();
        assert!(!is_installed(root.path(), EN_GB));
        install(root.path(), EN_GB).unwrap();
        assert!(load(root.path(), EN_GB).unwrap().check("colour"));
    }

    #[test]
    fn catalogue_has_unique_safe_package_locations() {
        assert!(package("en-gb").is_some());
        assert!(package("https://example.invalid").is_none());
        let root = tempfile::tempdir().unwrap();
        assert_eq!(package_dir(root.path(), EN_GB), root.path().join("en-gb"));
    }
}
