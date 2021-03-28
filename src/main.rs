use std::{
    error::Error,
    io::{stdout, Write},
    time::Duration,
};

use crossterm::{
    self,
    cursor::{Hide, MoveTo, MoveToColumn, MoveToNextLine, RestorePosition, SavePosition, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};
use xi_rope::{LinesMetric, Rope};

const FRAME_TOP_LEFT: char = '╔';
const FRAME_TOP_RIGHT: char = '╗';
const FRAME_BOTTOM_LEFT: char = '╚';
const FRAME_BOTTOM_RIGHT: char = '╝';
const HORIZONTAL: char = '═';
const VERTICAL: char = '║';

enum EditorCommand {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    Insert(char),
    InsertNewline,
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
                let offset = self.get_offset();
                match self.buffer.prev_grapheme_offset(offset) {
                    Some(new_offset) => {
                        self.cursor = self.offset_to_cursor(new_offset);
                    }
                    None => {
                        self.cursor.0 = 0;
                    }
                }
            }
            EditorCommand::MoveRight => {
                let offset = self.get_offset();
                match self.buffer.next_grapheme_offset(offset) {
                    Some(new_offset) => {
                        self.cursor = self.offset_to_cursor(new_offset);
                    }
                    _ => {}
                }
            }
            EditorCommand::MoveUp => {
                if self.cursor.1 == 0 {
                    self.cursor = (0, 0);
                } else {
                    let start_of_line = self.buffer.offset_of_line(self.cursor.1 - 1);
                    let line_length = self
                        .buffer
                        .lines(start_of_line..)
                        .next()
                        .map(|line| line.len())
                        .unwrap_or(0);

                    let new_offset = start_of_line + self.cursor.0.min(line_length);
                    let new_offset_aligned = self
                        .buffer
                        .at_or_prev_codepoint_boundary(new_offset)
                        .unwrap_or(0);

                    self.cursor = self.offset_to_cursor(new_offset_aligned);
                }
            }
            EditorCommand::MoveDown => {
                let lines = self.buffer.measure::<LinesMetric>();
                if lines == 0 || self.cursor.1 == lines - 1 {
                    let offset = self
                        .buffer
                        .at_or_next_codepoint_boundary(self.buffer.len())
                        .unwrap();
                    self.cursor = self.offset_to_cursor(offset);
                } else {
                    let start_of_line = self.buffer.offset_of_line(self.cursor.1 + 1);
                    let line_length = self
                        .buffer
                        .lines(start_of_line..)
                        .next()
                        .map(|line| line.len())
                        .unwrap_or(0);

                    let new_offset = start_of_line + self.cursor.0.min(line_length);
                    let new_offset_aligned = self
                        .buffer
                        .at_or_prev_codepoint_boundary(new_offset)
                        .unwrap_or(0);
                    self.cursor = self.offset_to_cursor(new_offset_aligned);
                }
            }
            EditorCommand::Insert(ch) => {
                let offset = self.get_offset();
                self.buffer.edit(offset..offset, String::from(ch));
                self.cursor.0 += ch.len_utf8();
            }
            EditorCommand::InsertNewline => {
                let offset = self.get_offset();
                self.buffer.edit(offset..offset, "\n");
                self.cursor.0 = 0;
                self.cursor.1 += 1;
            }
            EditorCommand::Remove => {
                let offset = self.get_offset();

                if offset == 0 {
                    return;
                }

                let start = self.buffer.prev_grapheme_offset(offset).unwrap_or(0);
                self.buffer.edit(start..offset, "");
                self.cursor = self.offset_to_cursor(start);
            }
        }
    }

    fn get_offset(&self) -> usize {
        self.buffer.offset_of_line(self.cursor.1) + self.cursor.0
    }

    fn offset_to_cursor(&self, offset: usize) -> (usize, usize) {
        let y = self.buffer.line_of_offset(offset);
        let line_start = self.buffer.offset_of_line(y);
        let x = offset - line_start;
        (x, y)
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
    stdout.queue(Hide)?;
    stdout.queue(MoveTo(1, 1))?;
    stdout.queue(Print(' '))?;

    let (screen_width, screen_height) = terminal::size()?;
    let terminal_columns = (screen_width - 2) as usize;
    let terminal_rows = (screen_height - 2) as usize;

    let lines = buffer.measure::<LinesMetric>();

    for (y, line) in buffer.lines(..).enumerate() {
        stdout.queue(MoveTo(1, 1 + y as u16))?;
        stdout.queue(Print(&line))?;
        let remaining = terminal_columns - line.chars().count();
        stdout.queue(Print(" ".repeat(remaining)))?;
    }

    let filler_line = " ".repeat(terminal_columns);

    for y in (lines + 1)..terminal_rows {
        stdout.queue(MoveTo(1, 1 + y as u16))?;
        stdout.queue(Print(&filler_line))?;
    }

    stdout.queue(RestorePosition)?;
    stdout.queue(Show)?;
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
                    execute!(stdout(), Clear(ClearType::All),)?;
                    draw_frame(&frame)?;
                    render_buffer(&state.buffer)?;
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
                    code: KeyCode::Up, ..
                }) => {
                    state.process(EditorCommand::MoveUp);
                    synchronize_cursor(state.get_cursor())?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    ..
                }) => {
                    state.process(EditorCommand::MoveDown);
                    synchronize_cursor(state.get_cursor())?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    modifiers: KeyModifiers::CONTROL,
                }) => {
                    break;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    state.process(EditorCommand::InsertNewline);
                    render_buffer(&state.buffer)?;
                    synchronize_cursor(state.get_cursor())?;
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

    execute!(stdout(), Clear(ClearType::All),)?;

    Ok(())
}
