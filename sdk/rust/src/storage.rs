use crate::{Error, host_imports};

pub const MAX_KEY_BYTES: usize = 64;
pub const MAX_VALUE_BYTES: usize = 8 * 1024;

pub fn put(key: &str, value: &[u8]) -> Result<(), Error> {
    if !valid_key(key) || value.is_empty() || value.len() > MAX_VALUE_BYTES {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_storage_put(
        key.as_ptr(),
        key.len() as u32,
        value.as_ptr(),
        value.len() as u32,
    ))
}

pub fn get(key: &str, value: &mut [u8]) -> Result<Option<usize>, Error> {
    if !valid_key(key) || value.is_empty() || value.len() > MAX_VALUE_BYTES {
        return Err(Error::InvalidArgument);
    }
    let result = host_imports::cp0_storage_get(
        key.as_ptr(),
        key.len() as u32,
        value.as_mut_ptr(),
        value.len() as u32,
    );
    if result < 0 {
        return Error::from_host(result).map(|()| None);
    }
    let length = result as usize;
    if length == 0 {
        Ok(None)
    } else if length <= value.len() {
        Ok(Some(length))
    } else {
        Err(Error::Internal)
    }
}

pub fn delete(key: &str) -> Result<bool, Error> {
    if !valid_key(key) {
        return Err(Error::InvalidArgument);
    }
    match host_imports::cp0_storage_delete(key.as_ptr(), key.len() as u32) {
        0 => Ok(false),
        1 => Ok(true),
        value if value < 0 => Error::from_host(value).map(|()| unreachable!()),
        _ => Err(Error::Internal),
    }
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
        && !key.starts_with('.')
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_keys_values_and_unavailable_host() {
        for key in ["", ".hidden", "../escape", "with/slash", "bad key"] {
            assert!(!valid_key(key), "accepted {key:?}");
        }
        assert!(valid_key("state.v1"));
        assert_eq!(put("state", b"value"), Err(Error::Unavailable));
        let mut value = [0_u8; 16];
        assert_eq!(get("state", &mut value), Err(Error::Unavailable));
        assert_eq!(delete("state"), Err(Error::Unavailable));
    }
}
