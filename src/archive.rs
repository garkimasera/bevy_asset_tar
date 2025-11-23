use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ArchiveFileKind {
    Tar,
    TarGz,
}

#[derive(Clone, Debug)]
pub struct ArchiveFileExtensionList(pub(crate) HashMap<String, ArchiveFileKind>);

impl Default for ArchiveFileExtensionList {
    fn default() -> Self {
        let mut list = HashMap::default();
        list.insert(".tar".into(), ArchiveFileKind::Tar);
        list.insert(".tar.gz".into(), ArchiveFileKind::TarGz);
        list.insert(".tgz".into(), ArchiveFileKind::TarGz);
        Self(list)
    }
}

impl ArchiveFileExtensionList {
    pub fn from_path(&self, path: &std::path::Path) -> Option<ArchiveFileKind> {
        self.0.iter().find_map(|(ext, kind)| {
            if let Some(file) = path.to_str() {
                if file.ends_with(ext) {
                    Some(*kind)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

#[derive(Debug)]
pub struct Archive {
    files: HashMap<PathBuf, Vec<u8>>,
    dirs: HashSet<PathBuf>,
}

impl Archive {
    pub fn new() -> Self {
        Self {
            files: HashMap::default(),
            dirs: HashSet::default(),
        }
    }

    pub fn append(&mut self, kind: ArchiveFileKind, input: Vec<u8>) -> Result<(), Error> {
        match kind {
            ArchiveFileKind::Tar => self.read_tar(input),
            ArchiveFileKind::TarGz => self.read_tar_gz(input),
        }
    }

    fn read_tar(&mut self, input: Vec<u8>) -> Result<(), Error> {
        let mut tar = tar::Archive::new(Cursor::new(input));

        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            let path = if let Ok(path) = path.strip_prefix("./") {
                path.to_owned()
            } else {
                path.into_owned()
            };
            if path.as_os_str().is_empty() {
                continue;
            }

            match entry.header().entry_type() {
                tar::EntryType::Regular => {
                    let mut file = Vec::new();
                    entry.read_to_end(&mut file)?;
                    if self.files.insert(path, file).is_some() {
                        log::info!(
                            "overwrite \"{}\"",
                            String::from_utf8_lossy(&entry.path_bytes())
                        );
                    }
                }
                tar::EntryType::Directory => {
                    self.dirs.insert(path);
                }
                t => {
                    return Err(Error::other(format!("Unexpected file type in tar {:?}", t)));
                }
            }
        }

        Ok(())
    }

    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error> {
        if let Some(data) = self.files.get(path).cloned() {
            Ok(data)
        } else {
            Err(Error::new(ErrorKind::NotFound, path.display().to_string()))
        }
    }

    pub fn read_dir(&self, path: &Path) -> Result<Dir, Error> {
        if !self.is_dir(path)? {
            return Err(Error::other(format!(
                "{} is not a directory",
                path.display()
            )));
        }

        let mut files = Vec::new();

        for p in self.files.keys() {
            if let Some(parent) = p.parent()
                && parent == path
            {
                files.push(p.to_owned())
            }
        }

        for p in &self.dirs {
            if let Some(parent) = p.parent()
                && parent == path
            {
                files.push(p.to_owned())
            }
        }

        Ok(Dir(files))
    }

    pub fn is_dir(&self, path: &Path) -> Result<bool, Error> {
        if self.dirs.contains(path) {
            Ok(true)
        } else if self.files.contains_key(path) {
            Ok(false)
        } else {
            Err(Error::new(ErrorKind::NotFound, path.display().to_string()))
        }
    }

    fn read_tar_gz(&mut self, input: Vec<u8>) -> Result<(), Error> {
        let mut decoded = Vec::new();
        let mut gz = flate2::read::GzDecoder::new(Cursor::new(input));
        gz.read_to_end(&mut decoded)?;
        self.read_tar(decoded)
    }
}

#[derive(Debug)]
pub struct Dir(Vec<PathBuf>);

impl bevy::tasks::futures_lite::Stream for Dir {
    type Item = PathBuf;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.get_mut().0.pop())
    }
}
