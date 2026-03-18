use std::{fs, hash::BuildHasher};

use crate::{color::Color, screen::ScreenBuffer};

#[derive(Default, Clone, Copy, Debug)]
pub struct CharBoundary {
    pub start_x: usize,
    pub start_y: usize,
    pub end_x: usize,
    pub end_y: usize,
}

#[derive(Clone, Debug)]
pub struct Font {
    pub font_size: f32,
    pub font: Vec<u8>,
    pub builded: fontdue::Font,
}

impl Font {
    pub fn new(font_dir: String, font_size: f32) -> Font {
        let file = fs::read(font_dir);
        match file {
            Ok(file) => Font {
                font_size,
                font: file.clone(),
                builded: fontdue::Font::from_bytes(file, fontdue::FontSettings::default()).unwrap(),
            },
            Err(e) => panic!("Failed to read font file ({})", e.to_string()),
        }
    }

    pub fn from_bytes(file: Vec<u8>, font_size: f32) -> Font {
        Font {
            font_size,
            font: file.clone(),
            builded: fontdue::Font::from_bytes(
                file,
                fontdue::FontSettings {
                    scale: font_size,
                    ..Default::default()
                },
            )
            .unwrap(),
        }
    }
}

pub struct Boundaries {
    pub start_x: usize,
    pub start_y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Char {
    pub rendered: bool,
    pub char: char,
    pub boundaries: Option<CharBoundary>,
}

impl Char {
    pub fn build(c: char) -> Char {
        Char {
            char: c,
            ..Default::default()
        }
    }
}

pub struct Cursor {
    pub font: Font,
    pub case: bool,
    pub buffer: Vec<Char>,
    pub pos: usize,
    pub blink: bool,
    pub boundaries: Boundaries,
    pub color: Color,
    pub background_color: Color,
}

impl Cursor {
    pub fn new(font: Font, boundaries: Boundaries) -> Cursor {
        Cursor {
            font,
            case: true,
            buffer: vec![],
            pos: 0,
            blink: true,
            boundaries,
            color: Color::from_rgb(255, 255, 255),
            background_color: Color::from_rgb(0, 0, 0),
        }
    }

    pub fn new_line(&mut self) {
        self.pos = 0;
        self.buffer.push(Char::build('\n'));
    }

    pub fn println(&mut self, text: &str) {
        self.print(text);
        self.new_line();
    }

    pub fn print(&mut self, text: &str) {
        for c in text.chars() {
            self.print_char(c);
        }
    }

    fn print_char(&mut self, c: char) {
        self.buffer.push(Char::build(c));
        self.pos += 1;
    }

    pub fn backspace(&mut self) {
        if self.pos > 0 {
            self.buffer.remove(self.pos - 1);
            self.pos -= 1;
        }
    }

    pub fn clear(&mut self, screen_buffer: &mut ScreenBuffer) {
        let old_color = self.color;
        self.color = self.background_color;
        self.render(screen_buffer);
        self.color = old_color;
        self.buffer.clear();
        self.pos = 0;
    }

    pub fn clean_buffer(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    pub fn render(&mut self, screen_buffer: &mut ScreenBuffer) {
        let mut rx = 0;
        let mut ry = 0;
        for char in &mut self.buffer {
            //screen_buffer.draw_rect(self.boundaries.start_x + rx, self.boundaries.start_y + ry, 8, 80, Color::rand());
            match char.char {
                '\n' => {
                    rx = 0;
                    ry += self.font.font_size as usize;
                }
                _ => {
                    screen_buffer.draw_char_transparent(
                        char.char,
                        self.boundaries.start_x + rx,
                        self.boundaries.start_y + ry,
                        self.color,
                        &self.font,
                    );
                    rx += 15;
                    char.rendered = true;
                }
            }
        }
    }
}
