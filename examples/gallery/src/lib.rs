#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    Error, camera,
    display::{self, Rect},
    input, photos,
    ui::{ButtonStyle, Canvas, color},
};

const KEY_ENTER: u16 = 28;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const FRAME_BYTES: usize = camera::FRAME_BYTES;

static mut FRAME: [u16; camera::PIXEL_COUNT] = [0; camera::PIXEL_COUNT];

#[derive(Clone, Copy)]
enum ViewStatus {
    Photo,
    Empty,
    Authorize,
    Denied,
    Damaged,
}

struct Gallery {
    photos: [photos::Photo; photos::MAX_PHOTOS],
    count: usize,
    selected: usize,
    status: ViewStatus,
    confirm_delete: bool,
    delete_selected: bool,
}

impl Gallery {
    const fn new() -> Self {
        Self {
            photos: [photos::Photo { id: 0 }; photos::MAX_PHOTOS],
            count: 0,
            selected: 0,
            status: ViewStatus::Empty,
            confirm_delete: false,
            delete_selected: false,
        }
    }

    fn refresh(&mut self, latest: bool) {
        match photos::list(&mut self.photos) {
            Ok(count) => {
                self.count = count;
                self.selected = if count == 0 {
                    0
                } else if latest {
                    count - 1
                } else {
                    self.selected.min(count - 1)
                };
                self.status = if count == 0 {
                    ViewStatus::Empty
                } else {
                    ViewStatus::Photo
                };
            }
            Err(Error::Unavailable) => self.status = ViewStatus::Authorize,
            Err(Error::Denied) => self.status = ViewStatus::Denied,
            Err(_) => self.status = ViewStatus::Damaged,
        }
    }

    fn move_left(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.selected + 1 < self.count {
            self.selected += 1;
        }
    }
}

fn pixels() -> &'static mut [u16] {
    unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), camera::PIXEL_COUNT)
    }
}

fn frame_bytes(pixels: &mut [u16]) -> &mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast(), FRAME_BYTES) }
}

fn render(gallery: &mut Gallery, pixels: &mut [u16]) {
    if matches!(gallery.status, ViewStatus::Photo)
        && photos::load_rgb565(gallery.photos[gallery.selected], pixels).is_err()
    {
        gallery.status = ViewStatus::Damaged;
    }
    let bytes = frame_bytes(pixels);
    let mut canvas = Canvas::new(bytes, display::WIDTH, display::IMMERSIVE_HEIGHT).unwrap();
    if !matches!(gallery.status, ViewStatus::Photo) {
        canvas.clear(color::BACKGROUND);
        canvas.fill_rect(
            Rect {
                x: 36,
                y: 34,
                width: 248,
                height: 100,
            },
            color::SURFACE,
        );
        canvas.stroke_rect(
            Rect {
                x: 36,
                y: 34,
                width: 248,
                height: 100,
            },
            color::ACCENT,
        );
        canvas.draw_text(91, 54, "GALLERY", color::TEXT, 3);
        let (label, x, label_color) = match gallery.status {
            ViewStatus::Empty => ("NO PHOTOS", 133, color::MUTED),
            ViewStatus::Authorize => ("AUTHORIZE PHOTOS", 112, color::WARNING),
            ViewStatus::Denied => ("ACCESS DENIED", 121, color::DANGER),
            _ => ("PHOTO DAMAGED", 121, color::DANGER),
        };
        canvas.draw_text(x, 111, label, label_color, 1);
        return;
    }

    canvas.fill_rect(
        Rect {
            x: 8,
            y: 8,
            width: 75,
            height: 17,
        },
        color::SURFACE,
    );
    canvas.draw_text(16, 13, "GALLERY", color::TEXT, 1);
    let mut position = [0_u8; 8];
    let text = format_position(gallery.selected + 1, gallery.count, &mut position);
    canvas.fill_rect(
        Rect {
            x: 262,
            y: 145,
            width: 50,
            height: 17,
        },
        color::SURFACE,
    );
    canvas.draw_text(269, 151, text, color::TEXT, 1);

    if gallery.confirm_delete {
        canvas.fill_rect(
            Rect {
                x: 44,
                y: 47,
                width: 232,
                height: 76,
            },
            color::SURFACE,
        );
        canvas.stroke_rect(
            Rect {
                x: 44,
                y: 47,
                width: 232,
                height: 76,
            },
            color::DANGER,
        );
        canvas.draw_text(100, 61, "DELETE PHOTO?", color::TEXT, 1);
        canvas.button(
            Rect {
                x: 61,
                y: 88,
                width: 88,
                height: 24,
            },
            "CANCEL",
            if gallery.delete_selected {
                ButtonStyle::SECONDARY
            } else {
                ButtonStyle::PRIMARY
            },
        );
        canvas.button(
            Rect {
                x: 171,
                y: 88,
                width: 88,
                height: 24,
            },
            "DELETE",
            if gallery.delete_selected {
                ButtonStyle::DANGER
            } else {
                ButtonStyle::SECONDARY
            },
        );
    }
}

fn format_position<'a>(current: usize, total: usize, output: &'a mut [u8; 8]) -> &'a str {
    let mut length = write_number(current, output, 0);
    output[length] = b'/';
    length += 1;
    length = write_number(total, output, length);
    unsafe { core::str::from_utf8_unchecked(&output[..length]) }
}

fn write_number(value: usize, output: &mut [u8], offset: usize) -> usize {
    if value >= 10 {
        output[offset] = b'0' + (value / 10) as u8;
        output[offset + 1] = b'0' + (value % 10) as u8;
        offset + 2
    } else {
        output[offset] = b'0' + value as u8;
        offset + 1
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = pixels();
    let mut gallery = Gallery::new();
    gallery.refresh(true);
    let mut dirty = true;

    loop {
        if dirty {
            render(&mut gallery, pixels);
            if display::present_rgb565(frame_bytes(pixels), &[]).is_ok() {
                dirty = false;
            }
        }
        match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed => {
                if gallery.confirm_delete {
                    match event.code {
                        KEY_LEFT => gallery.delete_selected = false,
                        KEY_RIGHT => gallery.delete_selected = true,
                        KEY_ENTER if gallery.delete_selected => {
                            let photo = gallery.photos[gallery.selected];
                            match photos::delete(photo) {
                                Ok(_) => gallery.refresh(false),
                                Err(Error::Unavailable) => gallery.status = ViewStatus::Authorize,
                                Err(Error::Denied) => gallery.status = ViewStatus::Denied,
                                Err(_) => gallery.status = ViewStatus::Damaged,
                            }
                            gallery.confirm_delete = false;
                        }
                        KEY_ENTER => gallery.confirm_delete = false,
                        _ => {}
                    }
                } else {
                    match event.code {
                        KEY_LEFT => gallery.move_left(),
                        KEY_RIGHT => gallery.move_right(),
                        KEY_ENTER if gallery.count > 0 => {
                            gallery.confirm_delete = true;
                            gallery.delete_selected = false;
                        }
                        KEY_ENTER => gallery.refresh(true),
                        _ => {}
                    }
                }
                dirty = true;
            }
            Ok(_) => {
                if matches!(gallery.status, ViewStatus::Authorize) {
                    gallery.refresh(true);
                    dirty = true;
                }
            }
            Err(_) => return 1,
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
