use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Archive {
    files: HashMap<PathBuf, Vec<u8>>,
    dirs: HashSet<PathBuf>,
}

impl Archive {
    pub fn new(input: Vec<u8>) -> Result<Self, Error> {
        let mut tar = tar::Archive::new(Cursor::new(input));

        let mut files = HashMap::default();
        let mut dirs = HashSet::default();

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
                    files.insert(path, file);
                }
                tar::EntryType::Directory => {
                    dirs.insert(path);
                }
                t => {
                    return Err(Error::new(
                        ErrorKind::Other,
                        format!("Unexpected file type in tar {:?}", t),
                    ));
                }
            }
        }

        Ok(Self { files, dirs })
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
            return Err(Error::new(
                ErrorKind::Other,
                format!("{} is not a directory", path.display()),
            ));
        }

        let mut files = Vec::new();

        for p in self.files.keys() {
            if let Some(parent) = p.parent() {
                if parent == path {
                    files.push(p.to_owned())
                }
            }
        }

        for p in &self.dirs {
            if let Some(parent) = p.parent() {
                if parent == path {
                    files.push(p.to_owned())
                }
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

    pub fn from_gz(input: Vec<u8>) -> Result<Self, Error> {
        let mut decoded = Vec::new();
        let mut gz = flate2::read::GzDecoder::new(Cursor::new(input));
        gz.read_to_end(&mut decoded)?;
        Self::new(decoded)
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
