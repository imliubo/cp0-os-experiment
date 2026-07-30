use crate::Error;

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
}

pub fn poll_key_event(timeout_milliseconds: u32) -> Result<Option<KeyEvent>, Error> {
    if timeout_milliseconds > MAX_POLL_MILLISECONDS {
        return Err(Error::InvalidArgument);
    }
    let mut wire = [0_u8; 8];
    match host::poll_key_event(
        wire.as_mut_ptr(),
        wire.len() as u32,
        timeout_milliseconds as i32,
    ) {
        0 => Ok(None),
        1 => Ok(Some(KeyEvent {
            code: u16::from_le_bytes([wire[0], wire[1]]),
            pressed: wire[2] != 0,
            repeated: wire[3] != 0,
            modifiers: wire[4],
        })),
        status => Error::from_host(status).map(|()| None),
    }
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "cardputerzero")]
    unsafe extern "C" {
        #[link_name = "cp0_poll_key_event"]
        fn raw_poll_key_event(event: *mut u8, event_bytes: u32, timeout_milliseconds: i32) -> i32;
    }

    pub fn poll_key_event(event: *mut u8, event_bytes: u32, timeout_milliseconds: i32) -> i32 {
        unsafe { raw_poll_key_event(event, event_bytes, timeout_milliseconds) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub fn poll_key_event(_event: *mut u8, _event_bytes: u32, _timeout_milliseconds: i32) -> i32 {
        0
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
}
