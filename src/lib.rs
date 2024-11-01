mod archive;

use std::{io::ErrorKind, path::Path, sync::Arc};

use archive::Archive;
use bevy::{
    asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceId, ErasedAssetReader, PathStream,
        Reader, VecReader,
    },
    prelude::*,
    tasks::futures_lite::AsyncReadExt,
};
use futures_util::lock::{MappedMutexGuard, Mutex, MutexGuard};

#[derive(Clone, Default, Debug)]
#[non_exhaustive]
pub struct AssetTarPlugin {}

impl Plugin for AssetTarPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSource::build().with_reader(|| {
                Box::new(TarAssetReader {
                    reader: AssetSource::get_default_reader("".to_string())(),
                    archive: Mutex::default(),
                })
            }),
        );
    }
}

struct TarAssetReader {
    reader: Box<dyn ErasedAssetReader>,
    archive: Mutex<Option<Archive>>,
}

impl TarAssetReader {
    async fn load(
        &self,
    ) -> Result<MappedMutexGuard<'_, Option<Archive>, Archive>, AssetReaderError> {
        let mut archive = self.archive.lock().await;
        let mut buf = Vec::new();

        if archive.is_none() {
            if let Ok(mut r) = self.reader.read(Path::new("assets.tar.gz")).await {
                r.read_to_end(&mut buf).await?;
                *archive = Some(Archive::from_gz(buf)?);
            } else {
                let mut r = self.reader.read(Path::new("assets.tar")).await?;
                r.read_to_end(&mut buf).await?;
                *archive = Some(Archive::new(buf)?);
            }
        }

        Ok(MutexGuard::map(archive, |archive| {
            archive.as_mut().unwrap()
        }))
    }
}

impl AssetReader for TarAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<Reader<'a>>, AssetReaderError> {
        let archive = self.load().await?;
        Ok(Box::new(VecReader::new(
            archive
                .read_file(path)
                .map_err(|e| to_asset_reader_err(e, path))?,
        )))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<Box<Reader<'a>>, AssetReaderError> {
        let archive = self.load().await?;
        let mut meta_path = path.to_owned();
        let mut extension = path.extension().unwrap_or_default().to_os_string();
        extension.push(".meta");
        meta_path.set_extension(extension);
        Ok(Box::new(VecReader::new(
            archive
                .read_file(&meta_path)
                .map_err(|e| to_asset_reader_err(e, &meta_path))?,
        )))
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
