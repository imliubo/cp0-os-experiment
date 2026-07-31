use crate::{Error, host_imports};

pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 32;
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 160;
pub const MAX_WAIT_MILLISECONDS: u32 = 1000;

pub fn monotonic_milliseconds() -> u64 {
    host_imports::cp0_monotonic_milliseconds()
}

pub fn wait_event(timeout_milliseconds: u32) -> Result<(), Error> {
    if timeout_milliseconds > MAX_WAIT_MILLISECONDS {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_wait_event(timeout_milliseconds as i32))
}

pub fn post_notification(title: &str, body: &str) -> Result<(), Error> {
    if title.is_empty()
        || title.chars().count() > MAX_NOTIFICATION_TITLE_CHARS
        || body.chars().count() > MAX_NOTIFICATION_BODY_CHARS
        || title.chars().chain(body.chars()).any(char::is_control)
    {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_post_notification(
        title.as_ptr(),
        title.len() as u32,
        body.as_ptr(),
        body.len() as u32,
    ))
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
