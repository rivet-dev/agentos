use anyhow::{bail, Context, Result};
use vbare::OwnedVersionedData;

use super::generated::v2;

pub const PACKAGE_MANIFEST_VERSION: u16 = 2;

pub enum PackageManifest {
    V2(v2::PackageManifest),
}

impl OwnedVersionedData for PackageManifest {
    type Latest = v2::PackageManifest;

    fn wrap_latest(latest: Self::Latest) -> Self {
        Self::V2(latest)
    }

    fn unwrap_latest(self) -> Result<Self::Latest> {
        match self {
            Self::V2(data) => Ok(data),
        }
    }

    fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
        match version {
            PACKAGE_MANIFEST_VERSION => Ok(Self::V2(serde_bare::from_slice(payload)?)),
            _ => bail!("invalid package manifest version: {version}"),
        }
    }

    fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
        match self {
            Self::V2(data) => serde_bare::to_vec(&data).map_err(Into::into),
        }
    }

    fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
        vec![|data| Ok(data)]
    }

    fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
        vec![|data| Ok(data)]
    }
}

pub enum MountIndex {
    V2(v2::MountIndex),
}

impl OwnedVersionedData for MountIndex {
    type Latest = v2::MountIndex;

    fn wrap_latest(latest: Self::Latest) -> Self {
        Self::V2(latest)
    }

    fn unwrap_latest(self) -> Result<Self::Latest> {
        match self {
            Self::V2(data) => Ok(data),
        }
    }

    fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
        match version {
            PACKAGE_MANIFEST_VERSION => Ok(Self::V2(serde_bare::from_slice(payload)?)),
            _ => bail!("invalid package mount index version: {version}"),
        }
    }

    fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
        match self {
            Self::V2(data) => serde_bare::to_vec(&data).map_err(Into::into),
        }
    }

    fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
        vec![|data| Ok(data)]
    }

    fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
        vec![|data| Ok(data)]
    }
}

/// Encode the latest package manifest with an embedded 2-byte schema version.
pub fn encode_package_manifest(manifest: v2::PackageManifest) -> Result<Vec<u8>> {
    PackageManifest::wrap_latest(manifest)
        .serialize_with_embedded_version(PACKAGE_MANIFEST_VERSION)
        .context("encode package manifest")
}

/// Decode a versioned package manifest payload into the latest schema variant.
pub fn decode_package_manifest(payload: &[u8]) -> Result<v2::PackageManifest> {
    PackageManifest::deserialize_with_embedded_version(payload).context("decode package manifest")
}

/// Encode the latest package mount index with an embedded 2-byte schema version.
pub fn encode_mount_index(index: v2::MountIndex) -> Result<Vec<u8>> {
    MountIndex::wrap_latest(index)
        .serialize_with_embedded_version(PACKAGE_MANIFEST_VERSION)
        .context("encode package mount index")
}

/// Decode a versioned package mount index payload into the latest schema variant.
pub fn decode_mount_index(payload: &[u8]) -> Result<v2::MountIndex> {
    MountIndex::deserialize_with_embedded_version(payload).context("decode package mount index")
}
