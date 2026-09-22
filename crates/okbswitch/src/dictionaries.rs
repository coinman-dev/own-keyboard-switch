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

/// A dictionary package the application knows how to download safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    pub id: &'static str,
    pub language: &'static str,
    pub label: &'static str,
    pub license: &'static str,
    repository: &'static str,
    revision: &'static str,
    aff_path: &'static str,
    aff_sha256: &'static str,
    dic_path: &'static str,
    dic_sha256: &'static str,
    notice_path: &'static str,
    notice_name: &'static str,
    notice_sha256: &'static str,
    encoding: DictionaryEncoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DictionaryEncoding {
    Utf8,
    Koi8R,
}

/// Optional British English spelling.  The built-in RU and US English
/// dictionaries remain available offline and do not need downloading.
pub const EN_GB: Package = Package {
    id: "en-gb",
    language: "en",
    label: "English (United Kingdom)",
    license: "LGPL; see README_en_GB.txt",
    repository: "LibreOffice/dictionaries",
    revision: "32b006a2c22a4ac7e8ed3f03346f7b3d85a970a4",
    aff_path: "en/en_GB.aff",
    aff_sha256: "0fd6ed120ef28957847d98ba5149b117e27116cf81b5aa36208453f6755a36fd",
    dic_path: "en/en_GB.dic",
    dic_sha256: "04e90f34f5263bf26780e9c4a442e9ad16584e227af49ddd1b3b21b01df5b29c",
    notice_path: "en/README_en_GB.txt",
    notice_name: "README_en_GB.txt",
    notice_sha256: "18b5833de60a52ffcb6e5b6b5048bb5a5d910acfecd5b096985b77058223880c",
    encoding: DictionaryEncoding::Utf8,
};

/// Expanded modern Russian dictionary, published under MPL-2.0.
pub const RU_MODERN: Package = Package {
    id: "ru-modern",
    language: "ru",
    label: "Russian (modern, expanded)",
    license: "MPL-2.0; see LICENSE.txt",
    repository: "Goudron/ru-spelling-dictionary",
    revision: "69a18ae079084f11569f5190ac2080289055ef5e",
    aff_path: "ru_RU.aff",
    aff_sha256: "e8dc652231a2c0c34b04d9c9acc63f801c20111865f05e57091c3dc81c786136",
    dic_path: "ru_RU.dic",
    dic_sha256: "b565654f9942fea6c5a7ac0f748e2fb0e33b49eff5f951720d5f7b0350d65b77",
    notice_path: "LICENSE",
    notice_name: "LICENSE.txt",
    notice_sha256: "968c8d1e8cb68f2799a3c183b20aeeb22a04b815a2f52da457b80400294ec1bb",
    encoding: DictionaryEncoding::Koi8R,
};

pub const PACKAGES: &[Package] = &[EN_GB, RU_MODERN];

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
    let decode = |data: &[u8], expected: &str| -> Result<String> {
        if format!("{:x}", Sha256::digest(data)) != expected {
            bail!("dictionary SHA-256 mismatch");
        }
        match package.encoding {
            DictionaryEncoding::Utf8 => Ok(std::str::from_utf8(data)?
                .trim_start_matches('\u{feff}')
                .to_string()),
            DictionaryEncoding::Koi8R => {
                let (text, _, malformed) = encoding_rs::KOI8_R.decode(data);
                if malformed {
                    bail!("dictionary contains malformed KOI8-R");
                }
                Ok(text.replacen("SET KOI8-R", "SET UTF-8", 1))
            }
        }
    };
    let aff = decode(aff, package.aff_sha256)?;
    let dic = decode(dic, package.dic_sha256)?;
    spellbook::Dictionary::new(&aff, &dic)
        .map_err(|err| anyhow::anyhow!("cannot parse Hunspell dictionary: {err}"))
}

/// Downloads, hashes and atomically installs a package.
pub fn install(root: &Path, package: Package) -> Result<()> {
    let aff = fetch(package, package.aff_path)?;
    let dic = fetch(package, package.dic_path)?;
    parse_checked(package, &aff, &dic)?;
    let notice = fetch(package, package.notice_path)?;
    checked_text(&notice, package.notice_sha256)?;
    // Validate the complete pair before touching installed files. Since this
    // catalogue is pinned, existing verified files are byte-for-byte identical.
    let directory = package_dir(root, package);
    fs::create_dir_all(&directory)?;
    for (name, data) in [
        (package.notice_name, &notice),
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

pub fn uninstall(root: &Path, package: Package) -> Result<()> {
    let directory = package_dir(root, package);
    if directory.is_dir() {
        fs::remove_dir_all(directory)?;
    }
    Ok(())
}

fn fetch(package: Package, source: &str) -> Result<Vec<u8>> {
    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/{source}",
        package.repository, package.revision
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
        for (package, word) in [(EN_GB, "colour"), (RU_MODERN, "привет")] {
            assert!(!is_installed(root.path(), package));
            install(root.path(), package).unwrap();
            assert!(is_installed(root.path(), package));
            assert!(load(root.path(), package).unwrap().check(word));
        }
        let file = package_dir(root.path(), EN_GB).join("dictionary.dic");
        fs::write(&file, "1\ncorrupt\n").unwrap();
        assert!(!is_installed(root.path(), EN_GB));
        install(root.path(), EN_GB).unwrap();
        assert!(load(root.path(), EN_GB).unwrap().check("colour"));
    }

    #[test]
    fn uninstall_only_removes_the_selected_catalogue_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(package_dir(root.path(), EN_GB)).unwrap();
        fs::create_dir_all(package_dir(root.path(), RU_MODERN)).unwrap();
        fs::write(package_dir(root.path(), EN_GB).join("keep"), "en").unwrap();
        fs::write(package_dir(root.path(), RU_MODERN).join("remove"), "ru").unwrap();
        uninstall(root.path(), RU_MODERN).unwrap();
        assert!(package_dir(root.path(), EN_GB).join("keep").is_file());
        assert!(!package_dir(root.path(), RU_MODERN).exists());
    }

    #[test]
    fn catalogue_has_unique_safe_package_locations() {
        assert!(package("en-gb").is_some());
        assert!(package("ru-modern").is_some());
        assert!(package("https://example.invalid").is_none());
        let root = tempfile::tempdir().unwrap();
        assert_eq!(package_dir(root.path(), EN_GB), root.path().join("en-gb"));
    }
}
