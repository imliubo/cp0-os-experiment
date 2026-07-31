use crate::{Error, host_imports};

pub const MAX_ACTION_BYTES: usize = 96;
pub const MAX_PAYLOAD_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    pub action_length: usize,
    pub payload_length: usize,
}

pub fn send(action: &str, payload: &[u8]) -> Result<(), Error> {
    if !valid_action(action) || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_intent_send(
        action.as_ptr(),
        action.len() as u32,
        payload.as_ptr(),
        payload.len() as u32,
    ))
}

pub fn take(action: &mut [u8], payload: &mut [u8]) -> Result<Option<Message>, Error> {
    if action.is_empty()
        || action.len() > MAX_ACTION_BYTES
        || payload.is_empty()
        || payload.len() > MAX_PAYLOAD_BYTES
    {
        return Err(Error::InvalidArgument);
    }
    let result = host_imports::cp0_intent_take(
        action.as_mut_ptr(),
        action.len() as u32,
        payload.as_mut_ptr(),
        payload.len() as u32,
    );
    if result < 0 {
        return Error::from_host(result as i32).map(|()| None);
    }
    if result == 0 {
        return Ok(None);
    }
    let packed = result as u64;
    let action_length = (packed >> 32) as usize;
    let payload_length = (packed & u64::from(u32::MAX)) as usize;
    if action_length == 0
        || action_length > action.len()
        || payload_length > payload.len()
        || core::str::from_utf8(&action[..action_length])
            .ok()
            .is_none_or(|value| !valid_action(value))
    {
        return Err(Error::Internal);
    }
    Ok(Some(Message {
        action_length,
        payload_length,
    }))
}

pub fn valid_action(action: &str) -> bool {
    if action.is_empty() || action.len() > MAX_ACTION_BYTES {
        return false;
    }
    let mut parts = 0_usize;
    for part in action.split('.') {
        parts += 1;
        if part.is_empty()
            || part.len() > 32
            || !part.as_bytes()[0].is_ascii_lowercase()
            || part.ends_with('-')
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return false;
        }
    }
    parts >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_actions_and_maps_unavailable_host() {
        for action in [
            "",
            "open",
            "Dev.cardputerzero.open",
            "dev.cardputerzero.bad_action",
            "dev..open",
        ] {
            assert!(!valid_action(action), "accepted {action:?}");
        }
        assert!(valid_action("dev.cardputerzero.documents.open"));
        assert_eq!(
            send("dev.cardputerzero.documents.open", b"document-7"),
            Err(Error::Unavailable)
        );
        let mut action = [0_u8; MAX_ACTION_BYTES];
        let mut payload = [0_u8; MAX_PAYLOAD_BYTES];
        assert_eq!(take(&mut action, &mut payload), Err(Error::Unavailable));
    }
}
