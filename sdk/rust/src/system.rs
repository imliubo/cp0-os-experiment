use crate::Error;

pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 32;
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 160;
pub const MAX_WAIT_MILLISECONDS: u32 = 1000;

pub fn monotonic_milliseconds() -> u64 {
    host::monotonic_milliseconds()
}

pub fn wait_event(timeout_milliseconds: u32) -> Result<(), Error> {
    if timeout_milliseconds > MAX_WAIT_MILLISECONDS {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host::wait_event(timeout_milliseconds as i32))
}

pub fn post_notification(title: &str, body: &str) -> Result<(), Error> {
    if title.is_empty()
        || title.chars().count() > MAX_NOTIFICATION_TITLE_CHARS
        || body.chars().count() > MAX_NOTIFICATION_BODY_CHARS
        || title.chars().chain(body.chars()).any(char::is_control)
    {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host::post_notification(
        title.as_ptr(),
        title.len() as u32,
        body.as_ptr(),
        body.len() as u32,
    ))
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "cardputerzero")]
    unsafe extern "C" {
        #[link_name = "cp0_monotonic_milliseconds"]
        fn raw_monotonic_milliseconds() -> u64;
        #[link_name = "cp0_wait_event"]
        fn raw_wait_event(timeout_milliseconds: i32) -> i32;
        #[link_name = "cp0_post_notification"]
        fn raw_post_notification(
            title: *const u8,
            title_length: u32,
            body: *const u8,
            body_length: u32,
        ) -> i32;
    }

    pub fn monotonic_milliseconds() -> u64 {
        unsafe { raw_monotonic_milliseconds() }
    }

    pub fn wait_event(timeout_milliseconds: i32) -> i32 {
        unsafe { raw_wait_event(timeout_milliseconds) }
    }

    pub fn post_notification(
        title: *const u8,
        title_length: u32,
        body: *const u8,
        body_length: u32,
    ) -> i32 {
        unsafe { raw_post_notification(title, title_length, body, body_length) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub const fn monotonic_milliseconds() -> u64 {
        0
    }

    pub const fn wait_event(_timeout_milliseconds: i32) -> i32 {
        -2
    }

    pub const fn post_notification(
        _title: *const u8,
        _title_length: u32,
        _body: *const u8,
        _body_length: u32,
    ) -> i32 {
        -2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_notification_before_host_call() {
        assert_eq!(post_notification("", "body"), Err(Error::InvalidArgument));
        assert_eq!(
            post_notification("title", "line\nbreak"),
            Err(Error::InvalidArgument)
        );
        assert_eq!(post_notification("title", "body"), Err(Error::Unavailable));
    }

    #[test]
    fn validates_wait_bound() {
        assert_eq!(
            wait_event(MAX_WAIT_MILLISECONDS + 1),
            Err(Error::InvalidArgument)
        );
        assert_eq!(wait_event(1), Err(Error::Unavailable));
    }
}
