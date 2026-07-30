use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Line {
    GroveFunction = 0,
    ExternalUsbFunction = 1,
    Grove5vPower = 2,
    External5vPower = 3,
}

pub fn read(line: Line) -> Result<bool, Error> {
    match host::read(line as u32) {
        0 => Ok(false),
        1 => Ok(true),
        value if value < 0 => Error::from_host(value).map(|()| unreachable!()),
        _ => Err(Error::Internal),
    }
}

pub fn write(line: Line, value: bool) -> Result<(), Error> {
    Error::from_host(host::write(line as u32, u32::from(value)))
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "cardputerzero")]
    unsafe extern "C" {
        #[link_name = "cp0_gpio_read"]
        fn raw_read(line: u32) -> i32;
        #[link_name = "cp0_gpio_write"]
        fn raw_write(line: u32, value: u32) -> i32;
    }

    pub fn read(line: u32) -> i32 {
        unsafe { raw_read(line) }
    }

    pub fn write(line: u32, value: u32) -> i32 {
        unsafe { raw_write(line, value) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub const fn read(_line: u32) -> i32 {
        -2
    }

    pub const fn write(_line: u32, _value: u32) -> i32 {
        -2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_only_fixed_lines_and_maps_unavailable_host() {
        assert_eq!(Line::GroveFunction as u32, 0);
        assert_eq!(Line::External5vPower as u32, 3);
        assert_eq!(read(Line::GroveFunction), Err(Error::Unavailable));
        assert_eq!(
            write(Line::ExternalUsbFunction, true),
            Err(Error::Unavailable)
        );
    }
}
