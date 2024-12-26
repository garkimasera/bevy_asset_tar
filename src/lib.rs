mod archive;

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use archive::Archive;
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceId, ErasedAssetReader, PathStream,
        Reader, VecReader,
    },
    prelude::*,
};
use futures_util::lock::{MappedMutexGuard, Mutex, MutexGuard};

pub use archive::{ArchiveFileExtensionList, ArchiveFileKind};

#[derive(Clone, Debug)]
pub struct AssetTarPlugin {
    pub archive_files: Vec<PathBuf>,
    pub archive_file_extension_list: ArchiveFileExtensionList,
    pub addon_directories: Vec<PathBuf>,
}

impl Default for AssetTarPlugin {
    fn default() -> Self {
        Self {
            archive_files: vec![PathBuf::from("assets.tar.gz")],
            archive_file_extension_list: ArchiveFileExtensionList::default(),
            addon_directories: Vec::new(),
        }
    }
}

impl Plugin for AssetTarPlugin {
    fn build(&self, app: &mut App) {
        let archive_files = self.archive_files.clone();
        let archive_file_extension_list = self.archive_file_extension_list.clone();
        let addon_directories = self.addon_directories.clone();

        app.register_asset_source(
            AssetSourceId::Default,
            AssetSource::build().with_reader(move || {
                Box::new(TarAssetReader {
                    archive_files: archive_files.clone(),
                    archive_file_extension_list: archive_file_extension_list.clone(),
                    addon_directories: addon_directories.clone(),
                    reader: AssetSource::get_default_reader("".to_string())(),
                    archive: Mutex::default(),
                })
            }),
        );
    }
}

struct TarAssetReader {
    archive_files: Vec<PathBuf>,
    archive_file_extension_list: ArchiveFileExtensionList,
    addon_directories: Vec<PathBuf>,
    reader: Box<dyn ErasedAssetReader>,
    archive: Mutex<Option<Archive>>,
}

impl TarAssetReader {
    async fn load(
        &self,
    ) -> Result<MappedMutexGuard<'_, Option<Archive>, Archive>, AssetReaderError> {
        let mut archive = self.archive.lock().await;

        if archive.is_none() {
            let mut loading = Archive::new();

            for file in &self.archive_files {
                let Some(kind) = self.archive_file_extension_list.from_path(file) else {
                    log::warn!("unknown extension for \"{}\"", file.display());
                    continue;
                };

                let mut buf = Vec::new();
                if let Ok(mut r) = self.reader.read(file).await {
                    r.read_to_end(&mut buf).await?;
                }
                if let Err(e) = loading.append(kind, buf) {
                    log::warn!("cannot read \"{}\": {}", file.display(), e);
                }
            }
            load_from_addon_dirs(
                &mut loading,
                &self.addon_directories,
                &self.archive_file_extension_list,
            )
            .await;
            *archive = Some(loading);
        }

        Ok(MutexGuard::map(archive, |archive| {
            archive.as_mut().unwrap()
        }))
    }
}

impl AssetReader for TarAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let archive = self.load().await?;
        Ok(VecReader::new(
            archive
                .read_file(path)
                .map_err(|e| to_asset_reader_err(e, path))?,
        ))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let archive = self.load().await?;
        let mut meta_path = path.to_owned();
        let mut extension = path.extension().unwrap_or_default().to_os_string();
        extension.push(".meta");
        meta_path.set_extension(extension);
        Ok(VecReader::new(
            archive
                .read_file(&meta_path)
                .map_err(|e| to_asset_reader_err(e, &meta_path))?,
        ))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        let archive = self.load().await?;
        Ok(Box::new(
            archive
                .read_dir(path)
                .map_err(|e| to_asset_reader_err(e, path))?,
        ))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        let archive = self.load().await?;
        archive
            .is_dir(path)
            .map_err(|e| to_asset_reader_err(e, path))
    }
}

fn to_asset_reader_err(e: std::io::Error, path: &Path) -> AssetReaderError {
    if e.kind() == ErrorKind::NotFound {
        AssetReaderError::NotFound(path.to_owned())
    } else {
        AssetReaderError::Io(Arc::new(e))
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn load_from_addon_dirs(
    loading: &mut Archive,
    dirs: &[PathBuf],
    archive_file_extension_list: &ArchiveFileExtensionList,
) {
    use bevy::tasks::futures_lite::StreamExt;

    for dir in dirs {
        let mut entries = match async_fs::read_dir(dir).await {
            Ok(path) => path,
            Err(e) => {
                log::warn!("cannot read directory \"{}\": {}", dir.display(), e);
                continue;
            }
        };

        loop {
            let entry = match entries.try_next().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(e) => {
                    log::warn!("cannot read directory \"{}\": {}", dir.display(), e);
                    break;
                }
            };
            let path = entry.path();
            let Some(kind) = archive_file_extension_list.from_path(&path) else {
                continue;
            };
            match async_fs::read(&path).await {
                Ok(bytes) => {
                    if let Err(e) = loading.append(kind, bytes) {
                        log::warn!("cannot read \"{}\": {}", path.display(), e);
                    }
                }
                Err(e) => {
                    log::warn!("cannot read \"{}\": {}", path.display(), e);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
async fn load_from_addon_dirs(_: &mut Archive, dirs: &[PathBuf], _: &ArchiveFileExtensionList) {
    assert!(dirs.is_empty(), "addon not supported");
}
