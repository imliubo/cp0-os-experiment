use crate::{Error, host_imports};

pub const MAX_POLL_MILLISECONDS: u32 = 1000;
pub const MODIFIER_SHIFT: u8 = 1 << 0;
pub const MODIFIER_CONTROL: u8 = 1 << 1;
pub const MODIFIER_ALT: u8 = 1 << 2;
pub const MODIFIER_SUPER: u8 = 1 << 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: u16,
    pub pressed: bool,
    pub repeated: bool,
    pub modifiers: u8,
    pub character: Option<u8>,
}

fn decode_key_event(wire: [u8; 8]) -> KeyEvent {
    KeyEvent {
        code: u16::from_le_bytes([wire[0], wire[1]]),
        pressed: wire[2] != 0,
        repeated: wire[3] != 0,
        modifiers: wire[4],
        character: match wire[5] {
            0 => None,
            value => Some(value),
        },
    }
}

pub fn poll_key_event(timeout_milliseconds: u32) -> Result<Option<KeyEvent>, Error> {
    if timeout_milliseconds > MAX_POLL_MILLISECONDS {
        return Err(Error::InvalidArgument);
    }
    let mut wire = [0_u8; 8];
    match host_imports::cp0_poll_key_event(
        wire.as_mut_ptr(),
        wire.len() as u32,
        timeout_milliseconds as i32,
    ) {
        0 => Ok(None),
        1 => Ok(Some(decode_key_event(wire))),
        status => Error::from_host(status).map(|()| None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_timeout_and_reports_no_native_event() {
        assert_eq!(poll_key_event(0), Ok(None));
        assert_eq!(
            poll_key_event(MAX_POLL_MILLISECONDS + 1),
            Err(Error::InvalidArgument)
        );
    }

    #[test]
    fn decodes_system_character_without_changing_wire_size() {
        let event = decode_key_event([30, 0, 1, 0, MODIFIER_SHIFT, b'A', 0, 0]);
        assert_eq!(
            event,
            KeyEvent {
                code: 30,
                pressed: true,
                repeated: false,
                modifiers: MODIFIER_SHIFT,
                character: Some(b'A'),
            }
        );
        assert_eq!(decode_key_event([1, 0, 0, 0, 0, 0, 0, 0]).character, None);
    }
}
