use crate::{Error, host_imports};

pub const MAX_URL_BYTES: usize = 1024;
pub const MAX_RESPONSE_BODY_BYTES: usize = 2048;
pub const MAX_RANGE_BODY_BYTES: usize = 8 * 1024;
pub const MAX_RESOURCE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body_length: usize,
}

pub fn http_get(url: &str, body: &mut [u8]) -> Result<HttpResponse, Error> {
    if !valid_url(url) || body.is_empty() || body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(Error::InvalidArgument);
    }
    let packed = host_imports::cp0_http_get(
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

pub fn http_get_range(url: &str, offset: u64, body: &mut [u8]) -> Result<HttpResponse, Error> {
    if !valid_url(url)
        || body.is_empty()
        || body.len() > MAX_RANGE_BODY_BYTES
        || offset
            .checked_add(body.len() as u64)
            .is_none_or(|end| end > MAX_RESOURCE_BYTES)
    {
        return Err(Error::InvalidArgument);
    }
    decode_response(
        host_imports::cp0_http_get_range(
            url.as_ptr(),
            url.len() as u32,
            offset,
            body.as_mut_ptr(),
            body.len() as u32,
        ),
        body.len(),
    )
}

fn decode_response(packed: i64, capacity: usize) -> Result<HttpResponse, Error> {
    if packed < 0 {
        if !(-5..=-1).contains(&packed) {
            return Err(Error::Internal);
        }
        return Error::from_host(packed as i32).map(|()| unreachable!());
    }
    let status_code = ((packed as u64) >> 32) as u16;
    let body_length = (packed as u64 & u64::from(u32::MAX)) as usize;
    if !(100..=599).contains(&status_code) || body_length > capacity {
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
