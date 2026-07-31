use crate::{Error, host_imports};

pub const MAX_PAYLOAD_BYTES: usize = 64;
pub const MAX_RECEIVE_TIMEOUT_MS: u32 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub rssi_dbm: i16,
    pub snr_quarter_db: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    pub length: usize,
    pub metadata: Metadata,
}

pub fn send(payload: &[u8]) -> Result<(), Error> {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_lora_send(
        payload.as_ptr(),
        payload.len() as u32,
    ))
}

pub fn receive(payload: &mut [u8], timeout_ms: u32) -> Result<Option<Packet>, Error> {
    if payload.is_empty()
        || payload.len() > MAX_PAYLOAD_BYTES
        || timeout_ms == 0
        || timeout_ms > MAX_RECEIVE_TIMEOUT_MS
    {
        return Err(Error::InvalidArgument);
    }
    let mut metadata = [0_u8; 4];
    let result = host_imports::cp0_lora_receive(
        payload.as_mut_ptr(),
        payload.len() as u32,
        metadata.as_mut_ptr(),
        metadata.len() as u32,
        timeout_ms,
    );
    if result < 0 {
        return Error::from_host(result).map(|()| None);
    }
    let length = result as usize;
    if length == 0 {
        return Ok(None);
    }
    if length > payload.len() || metadata[3] != 0 {
        return Err(Error::Internal);
    }
    Ok(Some(Packet {
        length,
        metadata: Metadata {
            rssi_dbm: i16::from_le_bytes([metadata[0], metadata[1]]),
            snr_quarter_db: metadata[2] as i8,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_operations_and_maps_unavailable_host() {
        assert_eq!(send(&[]), Err(Error::InvalidArgument));
        assert_eq!(
            send(&[0; MAX_PAYLOAD_BYTES + 1]),
            Err(Error::InvalidArgument)
        );
        assert_eq!(send(b"hello"), Err(Error::Unavailable));
        let mut payload = [0_u8; MAX_PAYLOAD_BYTES];
        assert_eq!(receive(&mut payload, 0), Err(Error::InvalidArgument));
        assert_eq!(receive(&mut payload, 10), Err(Error::Unavailable));
    }
}
