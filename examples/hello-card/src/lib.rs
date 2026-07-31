#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{
    Error, audio, camera, display, documents, gpio, input, network, radio, storage, system,
};

const FRAME_BYTES: usize =
    display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];
static mut NETWORK_BODY: [u8; network::MAX_RESPONSE_BODY_BYTES] =
    [0; network::MAX_RESPONSE_BODY_BYTES];
static mut AUDIO_SAMPLES: [i16; audio::MAX_FRAMES] = [0; audio::MAX_FRAMES];
static mut CAMERA_PIXELS: [u16; camera::PIXEL_COUNT] = [0; camera::PIXEL_COUNT];
static mut LORA_PAYLOAD: [u8; radio::MAX_PAYLOAD_BYTES] = [0; radio::MAX_PAYLOAD_BYTES];
const KEY_N: u16 = 49;
const KEY_D: u16 = 32;
const KEY_P: u16 = 25;
const KEY_R: u16 = 19;
const KEY_C: u16 = 46;
const KEY_G: u16 = 34;
const KEY_L: u16 = 38;
const KEY_S: u16 = 31;

fn prepare_frame() -> &'static mut [u8] {
    // The frame lives in the WASM data section rather than the 64 KiB call
    // stack. The Runtime validates its complete linear-memory range.
    let frame = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(FRAME).cast::<u8>(),
            FRAME_BYTES,
        )
    };
    for y in 0..usize::from(display::STANDARD_HEIGHT) {
        for x in 0..usize::from(display::WIDTH) {
            let border = x < 4
                || x >= usize::from(display::WIDTH) - 4
                || y < 4
                || y >= usize::from(display::STANDARD_HEIGHT) - 4;
            let pixel: u16 = if border {
                0xffff
            } else if y < 50 {
                0xf800
            } else if y < 100 {
                0x07e0
            } else {
                0x001f
            };
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = pixel as u8;
            frame[offset + 1] = (pixel >> 8) as u8;
        }
    }
    frame
}

fn show_key(frame: &mut [u8], code: u16) {
    let color = 0x001fu16 | ((code & 0x1f) << 11) | ((code & 0x3f) << 5);
    for y in 126..142 {
        for x in 280..312 {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color as u8;
            frame[offset + 1] = (color >> 8) as u8;
        }
    }
}

fn show_action_status(frame: &mut [u8], color: u16) {
    for y in 126..142 {
        for x in 8..48 {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color as u8;
            frame[offset + 1] = (color >> 8) as u8;
        }
    }
}

fn request_network(frame: &mut [u8]) {
    let body = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(NETWORK_BODY).cast::<u8>(),
            network::MAX_RESPONSE_BODY_BYTES,
        )
    };
    match network::http_get("https://example.com/", body) {
        Ok(response) if (200..=299).contains(&response.status_code) => {
            show_action_status(frame, 0x07e0);
            let _ = system::post_notification("Network ready", "HTTPS request completed");
        }
        Ok(_) | Err(Error::Denied) => show_action_status(frame, 0xf800),
        Err(Error::Unavailable) => show_action_status(frame, 0xffe0),
        Err(_) => show_action_status(frame, 0xf81f),
    }
}

fn request_document(frame: &mut [u8]) {
    let mut buffer = [0_u8; 32];
    match documents::open() {
        Ok(document) => {
            let result = document.read(0, &mut buffer);
            let _ = document.close();
            match result {
                Ok(count) if count > 0 => show_action_status(frame, 0x07e0),
                Ok(_) => show_action_status(frame, 0xffe0),
                Err(_) => show_action_status(frame, 0xf81f),
            }
        }
        Err(Error::Denied) => show_action_status(frame, 0xf800),
        Err(Error::Unavailable) => show_action_status(frame, 0xffe0),
        Err(_) => show_action_status(frame, 0xf81f),
    }
}

fn audio_buffer() -> &'static mut [i16] {
    unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(AUDIO_SAMPLES).cast::<i16>(),
            audio::MAX_FRAMES,
        )
    }
}

fn request_audio_playback(frame: &mut [u8]) {
    let samples = audio_buffer();
    for (index, sample) in samples.iter_mut().enumerate() {
        *sample = if index % 36 < 18 { 8192 } else { -8192 };
    }
    let color = match audio::play_pcm_s16le(samples) {
        Ok(()) => 0x07e0,
        Err(Error::Denied) => 0xf800,
        Err(Error::Unavailable) => 0xffe0,
        Err(_) => 0xf81f,
    };
    show_action_status(frame, color);
}

fn request_audio_capture(frame: &mut [u8]) {
    let color = match audio::capture_pcm_s16le(audio_buffer()) {
        Ok(count) if count > 0 => 0x07e0,
        Ok(_) => 0xffe0,
        Err(Error::Denied) => 0xf800,
        Err(Error::Unavailable) => 0xffe0,
        Err(_) => 0xf81f,
    };
    show_action_status(frame, color);
}

fn request_camera(frame: &mut [u8]) {
    let pixels = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(CAMERA_PIXELS).cast::<u16>(),
            camera::PIXEL_COUNT,
        )
    };
    match camera::capture_rgb565(pixels) {
        Ok(()) => {
            for y in 0..usize::from(display::STANDARD_HEIGHT) {
                for x in 0..usize::from(display::WIDTH) {
                    let pixel = pixels[y * usize::from(camera::WIDTH) + x].to_le_bytes();
                    let offset = (y * usize::from(display::WIDTH) + x) * 2;
                    frame[offset] = pixel[0];
                    frame[offset + 1] = pixel[1];
                }
            }
            show_action_status(frame, 0x07e0);
        }
        Err(Error::Denied) => show_action_status(frame, 0xf800),
        Err(Error::Unavailable) => show_action_status(frame, 0xffe0),
        Err(_) => show_action_status(frame, 0xf81f),
    }
}

fn request_gpio(frame: &mut [u8]) {
    let color = match gpio::read(gpio::Line::GroveFunction)
        .and_then(|value| gpio::write(gpio::Line::GroveFunction, !value))
    {
        Ok(()) => 0x07e0,
        Err(Error::Denied) => 0xf800,
        Err(Error::Unavailable) => 0xffe0,
        Err(_) => 0xf81f,
    };
    show_action_status(frame, color);
}

fn request_lora_receive(frame: &mut [u8]) {
    let payload = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(LORA_PAYLOAD).cast::<u8>(),
            radio::MAX_PAYLOAD_BYTES,
        )
    };
    let color = match radio::receive(payload, 250) {
        Ok(Some(packet)) if packet.length > 0 => 0x07e0,
        Ok(_) => 0xffe0,
        Err(Error::Denied) => 0xf800,
        Err(Error::Unavailable) => 0xffe0,
        Err(_) => 0xf81f,
    };
    show_action_status(frame, color);
}

fn request_private_storage(frame: &mut [u8]) {
    let color = match storage::put("hello.state", b"stored") {
        Ok(()) => 0x07e0,
        Err(Error::Unavailable) => 0xffe0,
        Err(_) => 0xf81f,
    };
    show_action_status(frame, color);
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    const TITLE: &str = "Hello Card";
    const BODY: &str = "Runtime host call is active";
    let mut posted = false;
    let mut rendered = false;
    let frame = prepare_frame();

    loop {
        if !rendered {
            match display::present_rgb565(frame, &[]) {
                Ok(()) => rendered = true,
                Err(Error::Unavailable | Error::ResourceLimit) => {}
                Err(_) => return 1,
            }
        }
        if !posted {
            match system::post_notification(TITLE, BODY) {
                Ok(()) => posted = true,
                Err(Error::Unavailable) => {}
                Err(_) => return 1,
            }
        }
        match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed => {
                if event.code == KEY_N {
                    request_network(frame);
                }
                if event.code == KEY_D {
                    request_document(frame);
                }
                if event.code == KEY_P {
                    request_audio_playback(frame);
                }
                if event.code == KEY_R {
                    request_audio_capture(frame);
                }
                if event.code == KEY_C {
                    request_camera(frame);
                }
                if event.code == KEY_G {
                    request_gpio(frame);
                }
                if event.code == KEY_L {
                    request_lora_receive(frame);
                }
                if event.code == KEY_S {
                    request_private_storage(frame);
                }
                show_key(frame, event.code);
                rendered = false;
            }
            Ok(_) => {}
            Err(Error::ResourceLimit) => {}
            Err(_) => return 1,
        }
    }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
