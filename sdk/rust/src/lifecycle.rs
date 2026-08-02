use crate::Error;

pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024;
pub const CHECKPOINT_EXPORT: &str = "cp0_app_checkpoint";
pub const RESTORE_EXPORT: &str = "cp0_app_restore";

pub fn validate_checkpoint(schema_version: u32, payload: &[u8]) -> Result<(), Error> {
    if schema_version == 0 || payload.len() > MAX_CHECKPOINT_BYTES {
        return Err(Error::InvalidArgument);
    }
    Ok(())
}

pub fn validate_restore_buffer(schema_version: u32, payload_length: u32) -> Result<usize, Error> {
    let length = usize::try_from(payload_length).map_err(|_| Error::InvalidArgument)?;
    if schema_version == 0 || length > MAX_CHECKPOINT_BYTES {
        return Err(Error::InvalidArgument);
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_checkpoint_abi_bounds() {
        assert_eq!(validate_checkpoint(1, &[1, 2, 3]), Ok(()));
        assert_eq!(validate_checkpoint(0, &[]), Err(Error::InvalidArgument));
        assert_eq!(
            validate_checkpoint(1, &[0; MAX_CHECKPOINT_BYTES + 1]),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            validate_restore_buffer(2, MAX_CHECKPOINT_BYTES as u32),
            Ok(MAX_CHECKPOINT_BYTES)
        );
    }
}
