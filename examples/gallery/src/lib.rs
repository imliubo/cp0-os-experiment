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
    photos: [photos::Photo; photos::LIST_PAGE_PHOTOS],
    page_start: u64,
    page_count: usize,
    total: u64,
    selected: u64,
    status: ViewStatus,
    confirm_delete: bool,
    delete_selected: bool,
}

impl Gallery {
    const fn new() -> Self {
        Self {
            photos: [photos::Photo { id: 0 }; photos::LIST_PAGE_PHOTOS],
            page_start: 0,
            page_count: 0,
            total: 0,
            selected: 0,
            status: ViewStatus::Empty,
            confirm_delete: false,
            delete_selected: false,
        }
    }

    fn refresh(&mut self, latest: bool) {
        match photos::count() {
            Ok(total) => {
                self.total = total;
                self.selected = if total == 0 {
                    0
                } else if latest {
                    total - 1
                } else {
                    self.selected.min(total - 1)
                };
                self.status = if total == 0 {
                    ViewStatus::Empty
                } else {
                    ViewStatus::Photo
                };
                if total != 0 && self.load_selected_page().is_err() {
                    self.status = ViewStatus::Damaged;
                }
            }
            Err(Error::Unavailable) => self.status = ViewStatus::Authorize,
            Err(Error::Denied) => self.status = ViewStatus::Denied,
            Err(_) => self.status = ViewStatus::Damaged,
        }
    }

    fn move_left(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.load_selected_page().is_err() {
                self.status = ViewStatus::Damaged;
            }
        }
    }

    fn move_right(&mut self) {
        if self.selected + 1 < self.total {
            self.selected += 1;
            if self.load_selected_page().is_err() {
                self.status = ViewStatus::Damaged;
            }
        }
    }

    fn load_selected_page(&mut self) -> Result<(), Error> {
        let page_size = photos::LIST_PAGE_PHOTOS as u64;
        let page_start = self.selected / page_size * page_size;
        if self.page_count != 0
            && page_start == self.page_start
            && self.selected - page_start < self.page_count as u64
        {
            return Ok(());
        }
        self.photos.fill(photos::Photo { id: 0 });
        let count = photos::list_page(page_start, &mut self.photos)?;
        if count == 0 || self.selected - page_start >= count as u64 {
            return Err(Error::Internal);
        }
        self.page_start = page_start;
        self.page_count = count;
        Ok(())
    }

    fn selected_photo(&self) -> Result<photos::Photo, Error> {
        let position = self.selected.saturating_sub(self.page_start);
        if position >= self.page_count as u64 {
            return Err(Error::Internal);
        }
        Ok(self.photos[position as usize])
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
        && gallery
            .selected_photo()
            .and_then(|photo| photos::load_rgb565(photo, pixels))
            .is_err()
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
    let mut position = [0_u8; 41];
    let text = format_position(gallery.selected + 1, gallery.total, &mut position);
    canvas.fill_rect(
        Rect {
            x: 212,
            y: 145,
            width: 100,
            height: 17,
        },
        color::SURFACE,
    );
    canvas.draw_text(220, 151, text, color::TEXT, 1);

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

fn format_position<'a>(current: u64, total: u64, output: &'a mut [u8; 41]) -> &'a str {
    let mut length = write_number(current, output, 0);
    output[length] = b'/';
    length += 1;
    length = write_number(total, output, length);
    unsafe { core::str::from_utf8_unchecked(&output[..length]) }
}

fn write_number(mut value: u64, output: &mut [u8], offset: usize) -> usize {
    if value == 0 {
        output[offset] = b'0';
        return offset + 1;
    }
    let mut length = offset;
    while value != 0 {
        output[length] = b'0' + (value % 10) as u8;
        value /= 10;
        length += 1;
    }
    output[offset..length].reverse();
    length
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
                            match gallery.selected_photo().and_then(photos::delete) {
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
                        KEY_ENTER if gallery.total > 0 => {
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
