use std::{
    error::Error,
    io::{stdout, Write},
    time::Duration,
};

use crossterm::{
    self,
    cursor::{MoveTo, MoveToColumn, MoveToNextLine, RestorePosition, SavePosition},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};
use xi_rope::Rope;

const FRAME_TOP_LEFT: char = '╔';
const FRAME_TOP_RIGHT: char = '╗';
const FRAME_BOTTOM_LEFT: char = '╚';
const FRAME_BOTTOM_RIGHT: char = '╝';
const HORIZONTAL: char = '═';
const VERTICAL: char = '║';

enum EditorCommand {
    MoveLeft,
    MoveRight,
    Insert(char),
    Remove,
}

struct BufferState {
    cursor: (usize, usize),
    buffer: Rope,
}

impl BufferState {
    pub fn new() -> BufferState {
        BufferState {
            cursor: (0, 0),
            buffer: Rope::default(),
        }
    }

    pub fn process(&mut self, command: EditorCommand) {
        match command {
            EditorCommand::MoveLeft => {
                let offset = self.cursor.0;
                match self.buffer.prev_grapheme_offset(offset) {
                    Some(new_offset) => {
                        self.cursor.0 = new_offset;
                    }
                    None => {
                        self.cursor.0 = 0;
                    }
                }
            }
            EditorCommand::MoveRight => {
                let offset = self.cursor.0;
                match self.buffer.next_grapheme_offset(offset) {
                    Some(new_offset) => {
                        self.cursor.0 = new_offset;
                    }
                    None => {}
                }
            }
            EditorCommand::Insert(ch) => {
                self.buffer
                    .edit(self.cursor.0..self.cursor.0, String::from(ch));
                self.cursor.0 += ch.len_utf8();
            }
            EditorCommand::Remove => {
                if self.cursor.0 == 0 {
                    return;
                }

                let start = self.buffer.prev_grapheme_offset(self.cursor.0).unwrap_or(0);
                self.buffer.edit(start..self.cursor.0, "");
                self.cursor.0 = start;
            }
        }
    }

    pub fn get_cursor(&self) -> (usize, usize) {
        let start_offset = self.buffer.line_of_offset(self.cursor.1);

        let x_bytes = self.cursor.0;

        let mut last_offset = None;

        for (x, (offset, _)) in self
            .buffer
            .slice_to_cow(start_offset..)
            .char_indices()
            .enumerate()
        {
            if offset == x_bytes {
                return (x, self.cursor.1);
            }

            last_offset = Some(x);
        }

        (last_offset.map(|x| x + 1).unwrap_or(0), self.cursor.1)
    }
}

#[derive(Debug)]
struct Frame {
    pub pos: (u16, u16),
    pub size: (u16, u16),
    pub title: Option<String>,
}

fn draw_frame(frame: &Frame) -> Result<(), Box<dyn Error>> {
    let pos = frame.pos;
    let size = frame.size;

    let max_x = size.0 - 1;
    let max_y = size.1 - 1;

    let mut stdout = stdout();

    stdout.queue(MoveTo(pos.0, pos.1))?;
    for y in 0..size.1 {
        stdout.queue(MoveToColumn(pos.0))?;
        for x in 0..size.0 {
            let ch = match (x, y) {
                (0, 0) => FRAME_TOP_LEFT,
                (x, 0) if x == max_x => FRAME_TOP_RIGHT,
                (_, 0) => HORIZONTAL,
                (0, y) if y == max_y => FRAME_BOTTOM_LEFT,
                (x, y) if x == max_x && y == max_y => FRAME_BOTTOM_RIGHT,
                (_, y) if y == max_y => HORIZONTAL,
                (0, _) => VERTICAL,
                (x, _) if x == max_x => VERTICAL,
                _ => ' ',
            };

            stdout.queue(Print(ch))?;
        }

        stdout.queue(MoveToNextLine(1))?;
    }

    if let Some(title) = &frame.title {
        let len = title.chars().count() as u16;
        let mid = pos.0 + (size.0 / 2);
        let text_start = mid - (len / 2);

        stdout.queue(MoveTo(text_start, pos.1))?;
        stdout.queue(Print(title))?;
    }

    stdout.queue(MoveTo(pos.0 + 1, pos.1 + 1))?;

    stdout.flush()?;

    Ok(())
}

fn render_buffer(buffer: &Rope) -> Result<(), Box<dyn Error>> {
    let mut stdout = stdout();

    stdout.queue(SavePosition)?;
    stdout.queue(MoveTo(1, 1))?;
    stdout.queue(Print(' '))?;

    for (i, line) in buffer.lines(..).enumerate() {
        stdout.queue(MoveTo(1, 1 + i as u16))?;
        stdout.queue(Print(&line))?;
        let remaining = (terminal::size()?.0 - 2) as usize - line.chars().count();
        stdout.queue(Print(" ".repeat(remaining)))?;
        stdout.flush()?;
    }

    stdout.queue(RestorePosition)?;
    stdout.flush()?;

    Ok(())
}

fn synchronize_cursor((x, y): (usize, usize)) -> Result<(), Box<dyn Error>> {
    stdout().execute(MoveTo(x as u16 + 1, y as u16 + 1))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut frame = Frame {
        pos: (0, 0),
        size: terminal::size()?,
        title: Some(String::from(" eminent ")),
    };

    execute!(stdout(), Clear(ClearType::All),)?;

    draw_frame(&frame)?;

    let mut state = BufferState::new();

    execute!(stdout(), MoveTo(1, 1),)?;

    render_buffer(&state.buffer)?;

    loop {
        let event_exists = event::poll(Duration::from_millis(10))?;

        if event_exists {
            let event = event::read()?;

            match event {
                Event::Resize(x, y) => {
                    frame.size = (x, y);
                    draw_frame(&frame)?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    ..
                }) => {
                    state.process(EditorCommand::MoveLeft);
                    synchronize_cursor(state.get_cursor())?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Right,
                    ..
                }) => {
                    state.process(EditorCommand::MoveRight);
                    synchronize_cursor(state.get_cursor())?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    modifiers: KeyModifiers::CONTROL,
                }) => {
                    break;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(ch),
                    ..
                }) => {
                    state.process(EditorCommand::Insert(ch));
                    render_buffer(&state.buffer)?;
                    synchronize_cursor(state.get_cursor())?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    state.process(EditorCommand::Remove);
                    render_buffer(&state.buffer)?;
                    synchronize_cursor(state.get_cursor())?;
                }
                _ => {}
            };
        }
    }

    Ok(())
}
