use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Text(String),
    Bytes { bytes: Vec<u8> },
}

impl Stored {
    fn of(path: &Path) -> Self {
        match path.to_str() {
            Some(text) => Self::Text(text.to_owned()),
            None => Self::Bytes {
                bytes: path.as_os_str().as_bytes().to_vec(),
            },
        }
    }

    fn into_path(self) -> PathBuf {
        match self {
            Self::Text(text) => PathBuf::from(text),
            Self::Bytes { bytes } => PathBuf::from(OsString::from_vec(bytes)),
        }
    }
}

pub(super) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    Stored::of(path).serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    Stored::deserialize(deserializer).map(Stored::into_path)
}

pub(super) mod many {
    use super::*;

    pub(in super::super) fn serialize<S: Serializer>(
        paths: &[PathBuf],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(paths.iter().map(|path| Stored::of(path)))
    }

    pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<PathBuf>, D::Error> {
        let stored = Vec::<Stored>::deserialize(deserializer)?;
        Ok(stored.into_iter().map(Stored::into_path).collect())
    }
}
