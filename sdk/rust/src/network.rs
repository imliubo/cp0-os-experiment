use crate::Error;

pub const MAX_URL_BYTES: usize = 1024;
pub const MAX_RESPONSE_BODY_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body_length: usize,
}

pub fn http_get(url: &str, body: &mut [u8]) -> Result<HttpResponse, Error> {
    if !valid_url(url) || body.is_empty() || body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(Error::InvalidArgument);
    }
    let packed = host::http_get(
        url.as_ptr(),
        url.len() as u32,
        body.as_mut_ptr(),
        body.len() as u32,
    );
    if packed < 0 {
        if !(-5..=-1).contains(&packed) {
            return Err(Error::Internal);
        }
        return Error::from_host(packed as i32).map(|()| unreachable!());
    }
    let status_code = ((packed as u64) >> 32) as u16;
    let body_length = (packed as u64 & u64::from(u32::MAX)) as usize;
    if !(100..=599).contains(&status_code) || body_length > body.len() {
        return Err(Error::Internal);
    }
    Ok(HttpResponse {
        status_code,
        body_length,
    })
}

fn valid_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() > "https://".len()
        && url.len() <= MAX_URL_BYTES
        && !url.chars().any(char::is_control)
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "cardputerzero")]
    unsafe extern "C" {
        #[link_name = "cp0_http_get"]
        fn raw_http_get(url: *const u8, url_length: u32, body: *mut u8, body_capacity: u32) -> i64;
    }

    pub fn http_get(url: *const u8, url_length: u32, body: *mut u8, body_capacity: u32) -> i64 {
        unsafe { raw_http_get(url, url_length, body, body_capacity) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub const fn http_get(
        _url: *const u8,
        _url_length: u32,
        _body: *mut u8,
        _body_capacity: u32,
    ) -> i64 {
        -2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_url_and_caller_owned_body_buffer() {
        let mut body = [0; MAX_RESPONSE_BODY_BYTES];
        assert_eq!(
            http_get("http://example.com", &mut body),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            http_get("https://example.com\n", &mut body),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            http_get("https://example.com", &mut []),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            http_get("https://example.com", &mut body),
            Err(Error::Unavailable)
        );
    }
}
