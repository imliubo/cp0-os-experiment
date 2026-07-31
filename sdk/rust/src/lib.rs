#![no_std]

pub mod audio;
pub mod camera;
pub mod display;
pub mod documents;
pub mod gpio;
mod host_imports;
pub mod input;
pub mod intents;
pub mod network;
pub mod radio;
pub mod storage;
pub mod system;
pub mod ui;

pub const SDK_VERSION_MAJOR: u32 = 1;
pub const SDK_VERSION_MINOR: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Denied,
    Unavailable,
    InvalidArgument,
    ResourceLimit,
    Internal,
}

impl Error {
    pub(crate) const fn from_host(value: i32) -> Result<(), Self> {
        match value {
            0 => Ok(()),
            -1 => Err(Self::Denied),
            -2 => Err(Self::Unavailable),
            -3 => Err(Self::InvalidArgument),
            -4 => Err(Self::ResourceLimit),
            _ => Err(Self::Internal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_private_host_status_to_stable_sdk_error() {
        assert_eq!((SDK_VERSION_MAJOR, SDK_VERSION_MINOR), (1, 0));
        assert_eq!(Error::from_host(0), Ok(()));
        assert_eq!(Error::from_host(-1), Err(Error::Denied));
        assert_eq!(Error::from_host(-2), Err(Error::Unavailable));
        assert_eq!(Error::from_host(-3), Err(Error::InvalidArgument));
        assert_eq!(Error::from_host(-4), Err(Error::ResourceLimit));
        assert_eq!(Error::from_host(-999), Err(Error::Internal));
    }

    #[test]
    fn public_wit_is_standards_compliant_sdk_1_0() {
        let mut resolve = wit_parser::Resolve::default();
        let package_id = resolve
            .push_source(
                "cardputerzero-sdk.wit",
                include_str!("../../wit/cardputerzero-sdk.wit"),
            )
            .expect("the public WIT contract must parse and resolve");
        let package = &resolve.packages[package_id];
        let version = package.name.version.as_ref().unwrap();

        assert_eq!(package.name.namespace, "cardputerzero");
        assert_eq!(package.name.name, "sdk");
        assert_eq!((version.major, version.minor, version.patch), (1, 0, 0));
        assert_eq!(package.interfaces.len(), 13);
        assert!(package.worlds.contains_key("cardputer-application"));
    }
}
