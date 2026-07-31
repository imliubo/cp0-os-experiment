#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    display::{self, Rect},
    storage,
    ui::{Canvas, color, rgb565},
};
#[cfg(not(test))]
use cp0_sdk::{input, system};

const KEY_ENTER: u16 = 28;
const KEY_R: u16 = 19;
const KEY_SPACE: u16 = 57;
const KEY_UP: u16 = 103;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_DOWN: u16 = 108;

const COLS: u8 = 30;
const ROWS: u8 = 11;
const MAX_CELLS: usize = COLS as usize * ROWS as usize;
const CELL_SIZE: u16 = 10;
const BOARD_X: u16 = 10;
const BOARD_Y: u16 = 30;
#[cfg(not(test))]
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
const BEST_SCORE_KEY: &str = "best-score.v1";

const FIELD_A: u16 = rgb565(13, 24, 30);
const FIELD_B: u16 = rgb565(16, 29, 35);
const GRID: u16 = rgb565(31, 49, 55);
const SNAKE_HEAD: u16 = rgb565(255, 214, 70);
const SNAKE_BODY: u16 = rgb565(30, 220, 180);
const SNAKE_ALT: u16 = rgb565(21, 165, 175);
const FOOD: u16 = rgb565(244, 74, 86);
const FOOD_SHINE: u16 = rgb565(255, 186, 152);
const LEAF: u16 = rgb565(111, 214, 88);

#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    x: u8,
    y: u8,
}

impl Cell {
    const ZERO: Self = Self { x: 0, y: 0 };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    const fn opposite(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Up, Self::Down)
                | (Self::Down, Self::Up)
                | (Self::Left, Self::Right)
                | (Self::Right, Self::Left)
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Paused,
    GameOver,
    Won,
}

struct Game {
    snake: [Cell; MAX_CELLS],
    length: usize,
    direction: Direction,
    next_direction: Direction,
    food: Cell,
    score: u32,
    best_score: u32,
    random: u32,
    state: State,
    next_tick: u64,
}

impl Game {
    fn new(now: u64, best_score: u32) -> Self {
        let mut game = Self {
            snake: [Cell::ZERO; MAX_CELLS],
            length: 4,
            direction: Direction::Right,
            next_direction: Direction::Right,
            food: Cell { x: 22, y: 5 },
            score: 0,
            best_score,
            random: 0x6d2b_79f5,
            state: State::Running,
            next_tick: now + 145,
        };
        game.snake[0] = Cell { x: 8, y: 5 };
        game.snake[1] = Cell { x: 7, y: 5 };
        game.snake[2] = Cell { x: 6, y: 5 };
        game.snake[3] = Cell { x: 5, y: 5 };
        game
    }

    fn handle_key(&mut self, code: u16, now: u64) -> bool {
        if matches!(self.state, State::GameOver | State::Won) && matches!(code, KEY_ENTER | KEY_R) {
            let best_score = self.best_score;
            *self = Self::new(now, best_score);
            return true;
        }
        if code == KEY_SPACE {
            self.state = match self.state {
                State::Running => State::Paused,
                State::Paused => {
                    self.next_tick = now + self.tick_milliseconds();
                    State::Running
                }
                state => state,
            };
            return true;
        }
        if self.state != State::Running {
            return false;
        }
        let requested = match code {
            KEY_UP => Some(Direction::Up),
            KEY_DOWN => Some(Direction::Down),
            KEY_LEFT => Some(Direction::Left),
            KEY_RIGHT => Some(Direction::Right),
            _ => None,
        };
        if let Some(direction) = requested {
            if !self.direction.opposite(direction) {
                self.next_direction = direction;
            }
            return true;
        }
        false
    }

    fn update(&mut self, now: u64) -> bool {
        if self.state != State::Running || now < self.next_tick {
            return false;
        }
        self.next_tick = now + self.tick_milliseconds();
        self.direction = self.next_direction;
        let head = self.snake[0];
        let next = match self.direction {
            Direction::Up if head.y > 0 => Cell {
                x: head.x,
                y: head.y - 1,
            },
            Direction::Down if head.y + 1 < ROWS => Cell {
                x: head.x,
                y: head.y + 1,
            },
            Direction::Left if head.x > 0 => Cell {
                x: head.x - 1,
                y: head.y,
            },
            Direction::Right if head.x + 1 < COLS => Cell {
                x: head.x + 1,
                y: head.y,
            },
            _ => {
                self.finish(State::GameOver);
                return true;
            }
        };

        let eating = next == self.food;
        let collision_length = self.length.saturating_sub(usize::from(!eating));
        if self.snake[..collision_length].contains(&next) {
            self.finish(State::GameOver);
            return true;
        }

        let next_length = if eating {
            (self.length + 1).min(MAX_CELLS)
        } else {
            self.length
        };
        for index in (1..next_length).rev() {
            self.snake[index] = self.snake[index - 1];
        }
        self.snake[0] = next;
        self.length = next_length;

        if eating {
            self.score = self.score.saturating_add(10);
            if self.score > self.best_score {
                self.best_score = self.score;
                let _ = storage::put(BEST_SCORE_KEY, &self.best_score.to_le_bytes());
            }
            if self.length == MAX_CELLS {
                self.finish(State::Won);
            } else {
                self.place_food();
            }
        }
        true
    }

    fn tick_milliseconds(&self) -> u64 {
        145_u64.saturating_sub(u64::from((self.score / 50).min(7)) * 10)
    }

    fn finish(&mut self, state: State) {
        self.state = state;
        if self.score > self.best_score {
            self.best_score = self.score;
            let _ = storage::put(BEST_SCORE_KEY, &self.best_score.to_le_bytes());
        }
    }

    fn place_food(&mut self) {
        for _ in 0..MAX_CELLS {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 17;
            self.random ^= self.random << 5;
            let candidate = Cell {
                x: (self.random % u32::from(COLS)) as u8,
                y: ((self.random / u32::from(COLS)) % u32::from(ROWS)) as u8,
            };
            if !self.snake[..self.length].contains(&candidate) {
                self.food = candidate;
                return;
            }
        }
        for y in 0..ROWS {
            for x in 0..COLS {
                let candidate = Cell { x, y };
                if !self.snake[..self.length].contains(&candidate) {
                    self.food = candidate;
                    return;
                }
            }
        }
    }
}

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

fn load_best_score() -> u32 {
    let mut bytes = [0_u8; 4];
    match storage::get(BEST_SCORE_KEY, &mut bytes) {
        Ok(Some(4)) => u32::from_le_bytes(bytes),
        _ => 0,
    }
}

fn render(game: &Game, pixels: &mut [u8]) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(10, 7, "NEON SNAKE", color::TEXT, 2);
    draw_number(&mut canvas, 181, 9, "SCORE", game.score, color::ACCENT);
    draw_number(&mut canvas, 255, 9, "BEST", game.best_score, color::WARNING);

    canvas.fill_rect(
        Rect {
            x: BOARD_X - 1,
            y: BOARD_Y - 1,
            width: u16::from(COLS) * CELL_SIZE + 2,
            height: u16::from(ROWS) * CELL_SIZE + 2,
        },
        GRID,
    );
    for y in 0..ROWS {
        for x in 0..COLS {
            canvas.fill_rect(
                Rect {
                    x: BOARD_X + u16::from(x) * CELL_SIZE,
                    y: BOARD_Y + u16::from(y) * CELL_SIZE,
                    width: CELL_SIZE - 1,
                    height: CELL_SIZE - 1,
                },
                if (x + y) % 2 == 0 { FIELD_A } else { FIELD_B },
            );
        }
    }

    draw_food(&mut canvas, game.food);
    for index in (0..game.length).rev() {
        draw_snake_cell(&mut canvas, game.snake[index], index, game.direction);
    }

    match game.state {
        State::Paused => draw_overlay(&mut canvas, "PAUSED", color::WARNING),
        State::GameOver => draw_overlay(&mut canvas, "GAME OVER", color::DANGER),
        State::Won => draw_overlay(&mut canvas, "BOARD CLEAR", color::SUCCESS),
        State::Running => {}
    }
}

fn draw_number(canvas: &mut Canvas<'_>, x: u16, y: u16, label: &str, value: u32, tint: u16) {
    canvas.draw_text(x, y, label, color::MUTED, 1);
    let mut buffer = [0_u8; 10];
    let number = format_u32(value, &mut buffer);
    canvas.draw_text(x, y + 9, number, tint, 1);
}

fn draw_food(canvas: &mut Canvas<'_>, cell: Cell) {
    let x = BOARD_X + u16::from(cell.x) * CELL_SIZE;
    let y = BOARD_Y + u16::from(cell.y) * CELL_SIZE;
    canvas.fill_rect(
        Rect {
            x: x + 2,
            y: y + 3,
            width: 6,
            height: 5,
        },
        FOOD,
    );
    canvas.fill_rect(
        Rect {
            x: x + 3,
            y: y + 2,
            width: 4,
            height: 7,
        },
        FOOD,
    );
    canvas.fill_rect(
        Rect {
            x: x + 3,
            y: y + 3,
            width: 2,
            height: 2,
        },
        FOOD_SHINE,
    );
    canvas.fill_rect(
        Rect {
            x: x + 6,
            y: y + 1,
            width: 3,
            height: 2,
        },
        LEAF,
    );
}

fn draw_snake_cell(canvas: &mut Canvas<'_>, cell: Cell, index: usize, direction: Direction) {
    let x = BOARD_X + u16::from(cell.x) * CELL_SIZE;
    let y = BOARD_Y + u16::from(cell.y) * CELL_SIZE;
    let tint = if index == 0 {
        SNAKE_HEAD
    } else if index % 2 == 0 {
        SNAKE_ALT
    } else {
        SNAKE_BODY
    };
    canvas.fill_rect(
        Rect {
            x: x + 1,
            y: y + 1,
            width: 7,
            height: 7,
        },
        tint,
    );
    if index != 0 {
        return;
    }
    let (eye_a, eye_b) = match direction {
        Direction::Up => ((x + 2, y + 2), (x + 6, y + 2)),
        Direction::Down => ((x + 2, y + 6), (x + 6, y + 6)),
        Direction::Left => ((x + 2, y + 2), (x + 2, y + 6)),
        Direction::Right => ((x + 6, y + 2), (x + 6, y + 6)),
    };
    canvas.pixel(eye_a.0, eye_a.1, color::BACKGROUND);
    canvas.pixel(eye_b.0, eye_b.1, color::BACKGROUND);
}

fn draw_overlay(canvas: &mut Canvas<'_>, label: &str, tint: u16) {
    canvas.fill_rect(
        Rect {
            x: 66,
            y: 70,
            width: 188,
            height: 30,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 66,
            y: 70,
            width: 188,
            height: 30,
        },
        tint,
    );
    let width = label.chars().count() as u16 * 12;
    canvas.draw_text(160_u16.saturating_sub(width / 2), 78, label, tint, 2);
}

fn format_u32(value: u32, buffer: &mut [u8; 10]) -> &str {
    let mut value = value;
    let mut cursor = buffer.len();
    if value == 0 {
        cursor -= 1;
        buffer[cursor] = b'0';
    }
    while value > 0 {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    unsafe { core::str::from_utf8_unchecked(&buffer[cursor..]) }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let now = system::monotonic_milliseconds();
    let mut game = Game::new(now, load_best_score());
    let mut dirty = true;
    loop {
        if dirty {
            render(&game, pixels);
            if display::present_rgb565(pixels, &[]).is_ok() {
                dirty = false;
            }
        }
        match input::poll_key_event(25) {
            Ok(Some(event)) if event.pressed => {
                dirty |= game.handle_key(event.code, system::monotonic_milliseconds());
            }
            Ok(_) => {}
            Err(_) => return 1,
        }
        dirty |= game.update(system::monotonic_milliseconds());
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_eats_and_updates_the_best_score() {
        let mut game = Game::new(0, 0);
        game.food = Cell { x: 9, y: 5 };

        assert!(game.update(145));
        assert!(game.snake[0] == Cell { x: 9, y: 5 });
        assert_eq!(game.length, 5);
        assert_eq!((game.score, game.best_score), (10, 10));
        assert!(game.state == State::Running);
    }

    #[test]
    fn rejects_an_immediate_reverse() {
        let mut game = Game::new(0, 0);

        assert!(game.handle_key(KEY_LEFT, 0));
        assert!(game.next_direction == Direction::Right);
        assert!(game.handle_key(KEY_UP, 0));
        assert!(game.next_direction == Direction::Up);
    }

    #[test]
    fn pauses_resumes_and_detects_a_wall() {
        let mut game = Game::new(0, 0);

        assert!(game.handle_key(KEY_SPACE, 10));
        assert!(game.state == State::Paused);
        assert!(!game.update(1_000));
        assert!(game.handle_key(KEY_SPACE, 1_000));
        assert!(game.state == State::Running);

        game.snake[0] = Cell { x: COLS - 1, y: 5 };
        assert!(game.update(1_145));
        assert!(game.state == State::GameOver);
    }

    #[test]
    fn renders_a_complete_standard_surface() {
        let game = Game::new(0, load_best_score());
        let mut pixels = [0_u8; display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2];

        render(&game, &mut pixels);

        assert!(pixels.iter().any(|byte| *byte != 0));
    }
}
