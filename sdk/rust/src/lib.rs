#![no_std]

pub mod display;
pub mod input;
pub mod system;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Denied,
    Unavailable,
    InvalidArgument,
    ResourceLimit,
    Internal,
}

impl Error {
    const fn from_host(value: i32) -> Result<(), Self> {
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
        assert_eq!(Error::from_host(0), Ok(()));
        assert_eq!(Error::from_host(-1), Err(Error::Denied));
        assert_eq!(Error::from_host(-2), Err(Error::Unavailable));
        assert_eq!(Error::from_host(-3), Err(Error::InvalidArgument));
        assert_eq!(Error::from_host(-4), Err(Error::ResourceLimit));
        assert_eq!(Error::from_host(-999), Err(Error::Internal));
    }
}
