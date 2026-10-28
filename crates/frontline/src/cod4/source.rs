//! Where the player's own CoD4 images live, and a names-only index over them (docs/adr/0004).
//!
//! A source path is one of:
//! - a CoD4 install (`main/iw_*.iwd`) or a folder holding `iw_*.iwd` archives,
//! - a zip that contains IWDs (e.g. a texture mod's `IW3_TEXTURES_MOD.zip`; nested zips are read
//!   in memory, one at a time),
//! - a zip (or an IWD) that holds `images/*.iwi` directly, e.g. `raw/images/*.iwi`.
//!
//! Indexing reads names only; later archives override earlier ones, like the game (`iw_06` over
//! `iw_00`). Bytes are read on demand and never written anywhere.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A zip on disk (`member: None`), or a zip stored as an entry of one.
#[derive(Clone, Debug)]
struct Archive {
    file: PathBuf,
    member: Option<String>,
}

impl Archive {
    fn open(&self) -> Result<ZipArchive<Box<dyn ReadSeek>>, String> {
        let file =
            File::open(&self.file).map_err(|err| format!("{}: {err}", self.file.display()))?;
        let Some(member) = &self.member else {
            return open_zip(
                Box::new(BufReader::new(file)),
                &self.file.display().to_string(),
            );
        };
        let label = format!("{}: {member}", self.file.display());
        let mut outer =
            ZipArchive::new(BufReader::new(file)).map_err(|err| format!("{label}: {err}"))?;
        let mut bytes = Vec::new();
        outer
            .by_name(member)
            .and_then(|mut entry| entry.read_to_end(&mut bytes).map_err(Into::into))
            .map_err(|err| format!("{label}: {err}"))?;
        open_zip(Box::new(Cursor::new(bytes)), &label)
    }
}

fn open_zip(
    reader: Box<dyn ReadSeek>,
    label: &str,
) -> Result<ZipArchive<Box<dyn ReadSeek>>, String> {
    ZipArchive::new(reader).map_err(|err| format!("{label}: {err}"))
}

/// Every image of a source, by lowercase name (no folder, no `.iwi`).
#[derive(Debug)]
pub struct Cod4Index {
    archives: Vec<Archive>,
    /// Image name -> (archive, zip entry name).
    images: HashMap<String, (usize, String)>,
    /// Archives that contributed at least one image.
    archive_count: usize,
}

impl Cod4Index {
    /// Indexes the source at `path`; names only, no image is read.
    pub fn scan(path: &Path) -> Result<Self, String> {
        let archives = if path.is_dir() {
            folder_archives(path)?
        } else {
            file_archives(path)?
        };
        let mut index = Cod4Index {
            archives: Vec::new(),
            images: HashMap::new(),
            archive_count: 0,
        };
        for (archive, opened) in archives {
            // The zip given directly was opened to list its members; reuse it for its images.
            let mut zip = match opened {
                Some(zip) => zip,
                None => archive.open()?,
            };
            index.add_archive(archive, &mut zip)?;
        }
        if index.images.is_empty() {
            return Err(format!(
                "no CoD4 images (*.iwi) found in {}",
                path.display()
            ));
        }
        Ok(index)
    }

    fn add_archive(
        &mut self,
        archive: Archive,
        zip: &mut ZipArchive<Box<dyn ReadSeek>>,
    ) -> Result<(), String> {
        let id = self.archives.len();
        let mut found = false;
        for name in zip.file_names() {
            let name = name.map_err(|err| err.to_string())?;
            if let Some(image) = image_name(&name) {
                self.images.insert(image, (id, name.into_owned()));
                found = true;
            }
        }
        if found {
            self.archive_count += 1;
        }
        self.archives.push(archive);
        Ok(())
    }

    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    pub fn archive_count(&self) -> usize {
        self.archive_count
    }

    /// The raw bytes of each requested image that exists, opening each archive once (the
    /// bytes live in memory only). Image names are matched case-insensitively and come back
    /// as requested.
    pub fn read_many(&self, wanted: &[&str]) -> Result<HashMap<String, Vec<u8>>, String> {
        let mut by_archive: HashMap<usize, Vec<(&str, &str)>> = HashMap::new();
        for &image in wanted {
            if let Some((archive, entry)) = self.images.get(&image.to_ascii_lowercase()) {
                by_archive.entry(*archive).or_default().push((image, entry));
            }
        }
        let mut found = HashMap::new();
        for (archive, entries) in by_archive {
            let mut zip = self.archives[archive].open()?;
            for (image, entry) in entries {
                let mut bytes = Vec::new();
                zip.by_name(entry)
                    .and_then(|mut file| file.read_to_end(&mut bytes).map_err(Into::into))
                    .map_err(|err| format!("{image}: {err}"))?;
                found.insert(image.to_owned(), bytes);
            }
        }
        Ok(found)
    }
}

/// `images/foo.iwi` -> `foo`; any folder in front of `images/` (e.g. `raw/`) is ignored.
fn image_name(entry: &str) -> Option<String> {
    let lower = entry.to_ascii_lowercase().replace('\\', "/");
    let stem = lower.strip_suffix(".iwi")?;
    let at = stem.rfind("images/")?;
    let name = &stem[at + "images/".len()..];
    (!name.is_empty() && !name.contains('/')).then(|| name.to_owned())
}

type Opened = Option<ZipArchive<Box<dyn ReadSeek>>>;

/// A CoD4 install (`main/`) or folder of `iw_*.iwd`, in override order.
fn folder_archives(dir: &Path) -> Result<Vec<(Archive, Opened)>, String> {
    let main = dir.join("main");
    let search = if main.is_dir() {
        main
    } else {
        dir.to_path_buf()
    };
    let entries =
        std::fs::read_dir(&search).map_err(|err| format!("{}: {err}", search.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_ascii_lowercase)
                .is_some_and(|name| name.starts_with("iw_") && name.ends_with(".iwd"))
        })
        .collect();
    files.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
    if files.is_empty() {
        return Err(format!("no iw_*.iwd archives in {}", search.display()));
    }
    Ok(files
        .into_iter()
        .map(|file| (Archive { file, member: None }, None))
        .collect())
}

/// A zip: its own `images/*.iwi` first, then the IWDs inside it in name order.
fn file_archives(path: &Path) -> Result<Vec<(Archive, Opened)>, String> {
    let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let zip = open_zip(Box::new(BufReader::new(file)), &path.display().to_string())?;
    let mut nested: Vec<String> = Vec::new();
    for name in zip.file_names() {
        let name = name.map_err(|err| err.to_string())?;
        if name.to_ascii_lowercase().ends_with(".iwd") {
            nested.push(name.into_owned());
        }
    }
    nested.sort_by_key(|name| name.to_ascii_lowercase());
    let mut archives = vec![(
        Archive {
            file: path.to_path_buf(),
            member: None,
        },
        Some(zip),
    )];
    archives.extend(nested.into_iter().map(|member| {
        (
            Archive {
                file: path.to_path_buf(),
                member: Some(member),
            },
            None,
        )
    }));
    Ok(archives)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_drop_folder_and_extension() {
        assert_eq!(
            image_name("images/Brick_01.iwi").as_deref(),
            Some("brick_01")
        );
        assert_eq!(image_name("raw/images/a_col.IWI").as_deref(), Some("a_col"));
        assert_eq!(image_name("images\\x.iwi").as_deref(), Some("x"));
        assert_eq!(image_name("sound/a.wav"), None);
        assert_eq!(image_name("images/sub/a.iwi"), None);
        assert_eq!(image_name("a.iwi"), None);
    }
}
