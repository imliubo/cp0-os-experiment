use crate::Error;

pub const MAX_DOCUMENT_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_READ_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub struct Document {
    handle: i32,
    length: u32,
}

impl Document {
    pub const fn len(&self) -> u32 {
        self.length
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, Error> {
        if buffer.is_empty() || buffer.len() > MAX_READ_BYTES {
            return Err(Error::InvalidArgument);
        }
        let result = host::read(
            self.handle,
            offset,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        );
        if result < 0 {
            return decode_error(result);
        }
        let count = result as usize;
        if count > buffer.len() {
            return Err(Error::Internal);
        }
        Ok(count)
    }

    pub fn close(self) -> Result<(), Error> {
        Error::from_host(host::close(self.handle))
    }
}

pub fn open() -> Result<Document, Error> {
    let packed = host::open();
    if packed < 0 {
        return decode_error(packed);
    }
    let handle = ((packed as u64) >> 32) as i32;
    let length = packed as u32;
    if handle <= 0 || length > MAX_DOCUMENT_BYTES {
        return Err(Error::Internal);
    }
    Ok(Document { handle, length })
}

fn decode_error<T>(value: i64) -> Result<T, Error> {
    if !(-5..=-1).contains(&value) {
        return Err(Error::Internal);
    }
    match Error::from_host(value as i32) {
        Ok(()) => unreachable!(),
        Err(error) => Err(error),
    }
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "cardputerzero")]
    unsafe extern "C" {
        #[link_name = "cp0_document_open"]
        fn raw_open() -> i64;
        #[link_name = "cp0_document_read"]
        fn raw_read(handle: i32, offset: u64, buffer: *mut u8, capacity: u32) -> i64;
        #[link_name = "cp0_document_close"]
        fn raw_close(handle: i32) -> i32;
    }

    pub fn open() -> i64 {
        unsafe { raw_open() }
    }

    pub fn read(handle: i32, offset: u64, buffer: *mut u8, capacity: u32) -> i64 {
        unsafe { raw_read(handle, offset, buffer, capacity) }
    }

    pub fn close(handle: i32) -> i32 {
        unsafe { raw_close(handle) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub const fn open() -> i64 {
        -2
    }

    pub const fn read(_handle: i32, _offset: u64, _buffer: *mut u8, _capacity: u32) -> i64 {
        -2
    }

    pub const fn close(_handle: i32) -> i32 {
        -2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_unavailable_and_buffer_bounds_are_stable() {
        assert_eq!(open(), Err(Error::Unavailable));
        let document = Document {
            handle: 1,
            length: 10,
        };
        assert_eq!(document.read(0, &mut []), Err(Error::InvalidArgument));
        let mut oversized = [0_u8; MAX_READ_BYTES + 1];
        assert_eq!(
            document.read(0, &mut oversized),
            Err(Error::InvalidArgument)
        );
        let mut bounded = [0_u8; 8];
        assert_eq!(document.read(0, &mut bounded), Err(Error::Unavailable));
    }
}
