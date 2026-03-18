use std::io::{Read, Write};

#[derive(Default, Clone, Copy, Debug)]
pub struct CharBoundary {
    pub start_x: usize,
    pub start_y: usize,
    pub end_x: usize,
    pub end_y: usize,
}

use crate::{color::Color, cursor::Font, performance_track::PerformanceTracker};

#[derive(Clone, Debug)]
pub struct ScreenBuffer {
    pub height: usize,
    pub width: usize,
    pub buffer: Vec<u32>,
}

impl Read for ScreenBuffer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut i = 0;
        for b in buf.iter_mut() {
            *b = self.buffer[i] as u8;
            i += 1;
        }
        Ok(buf.len())
    }
}

impl Write for ScreenBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut i = 0;
        for b in buf.iter() {
            self.buffer[i] = *b as u32;
            i += 1;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct Boundaries {
    pub start_x: usize,
    pub start_y: usize,
    pub width: usize,
    pub height: usize,
}

impl ScreenBuffer {
    pub fn new(width: usize, height: usize) -> ScreenBuffer {
        ScreenBuffer {
            width,
            height,
            buffer: vec![0; width * height],
        }
    }

    pub fn calc_buf_pos(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn draw_image(&mut self, image: Vec<u8>, w: usize, h: usize, color_type: u8) {
        let mut px = 0;
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w) as usize;
                let idy = (idx + x as usize) + px;
                let r = idy;
                let g = idy + 1;
                let b = idy + 2;
                let a = if color_type == 0 { 255 } else { idy + 3 };
                px += if color_type == 0 { 2 } else { 3 };
                let pixel = if color_type == 0 {
                    vec![image[r], image[g], image[b]]
                } else {
                    vec![image[r], image[g], image[b], image[a]]
                };

                ////let pixel = img_data[c];
                ////let pixel = rgbchunks.next().unwrap();

                let color = if color_type == 0 {
                    Color::from_rgb(pixel[0] as u32, pixel[1] as u32, pixel[2] as u32)
                } else {
                    Color::from_rgba(
                        pixel[0] as u32,
                        pixel[1] as u32,
                        pixel[2] as u32,
                        pixel[3] as u32,
                    )
                };

                if color_type == 0 {
                    self.put_pixel(x as usize, y as usize, color);
                } else {
                    self.put_pixel_a(x + 1 as usize, y + 1 as usize, color);
                }
            }
        }
    }

    pub fn draw_line(
        &mut self,
        start_x: usize,
        start_y: usize,
        end_x: usize,
        end_y: usize,
        color: Color,
    ) {
        let dx = end_x - start_x;
        let dy = end_y - start_y;
        for x in start_x..end_x {
            let y = start_y + dy * (x - start_x) / dx;
            self.put_pixel(x, y, Color::from_rgb(color.red, color.green, color.blue));
        }
        for y in start_y..end_y {
            let x = start_y + dx * (y - start_x) / dy;
            self.put_pixel(x, y, Color::from_rgb(color.red, color.green, color.blue));
        }
    }

    pub fn draw_rect(
        &mut self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: Color,
    ) {
        //self.draw_line(start_x, start_y, start_x + width, start_y, color.clone()); //TOP
        //self.draw_line(start_x, start_y, start_x, start_y + height, color); //LEFT

        for y in start_y..(start_y + height) {
            for x in start_x..(start_x + width) {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn draw_rect_alpha(
        &mut self,
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
        color: Color, // The alpha channel here (0-255) determines transparency
    ) {
        let alpha = color.alpha as u8;
        let fg_u32 = color.to_u32_argb();

        for y in start_y..(start_y + height) {
            for x in start_x..(start_x + width) {
                if x < self.width && y < self.height {
                    let index = self.calc_buf_pos(x, y);
                    let bg_pixel = self.buffer[index];

                    // Mix the background with our chart background color
                    self.buffer[index] = self.blend_pixels(bg_pixel, fg_u32, alpha);
                }
            }
        }
    }

    pub fn draw_char_transparent(
        &mut self,
        c: char,
        x: usize,
        y: usize,
        color: Color,
        font: &Font,
    ) {
        let (metrics, bitmap) = font.builded.rasterize(c, font.font_size);

        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let font_alpha = bitmap[row * metrics.width + col];

                if font_alpha > 0 {
                    let px = x + col + metrics.xmin as usize;
                    let py = y
                        + row
                        + (font.font_size as usize - metrics.height - metrics.ymin as usize);

                    if px < self.width && py < self.height {
                        let index = py * self.width + px;

                        // 1. Get the existing background pixel (the video)
                        let bg_pixel = self.buffer[index];

                        // 2. Get your cursor color as u32
                        let fg_pixel = color.to_u32_argb();

                        // 3. Blend them!
                        // Note: We use 'font_alpha' from fontdue to handle the edges
                        self.buffer[index] = self.blend_pixels(bg_pixel, fg_pixel, font_alpha);
                    }
                }
            }
        }
    }

    fn blend_pixels(&self, bg: u32, fg: u32, alpha: u8) -> u32 {
        if alpha == 255 {
            return fg;
        }
        if alpha == 0 {
            return bg;
        }

        let a = alpha as u32;
        let inv_a = 255 - a;

        // Fast blending for R and B at the same time
        let rb = ((fg & 0xFF00FF) * a + (bg & 0xFF00FF) * inv_a) >> 8;
        // Fast blending for G (and A if you want)
        let g = ((fg & 0x00FF00) * a + (bg & 0x00FF00) * inv_a) >> 8;

        (rb & 0xFF00FF) | (g & 0x00FF00)
    }

    pub fn draw_char(
        &mut self,
        chr: char,
        x: usize,
        y: usize,
        color: Color,
        background_color: Color,
        font: &Font,
    ) {
        let (metrics, bitmap) = font.builded.rasterize(chr, font.font_size);
        let mut current_x = x;
        let mut current_y = y;

        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let char_s = bitmap[x + y * metrics.width];

                let mut char_color = Color::from_rgb(char_s as u32, char_s as u32, char_s as u32);

                if char_color.red != 0 && char_color.green != 0 && char_color.blue != 0 {
                    char_color = color
                } else if char_color.red == 0 && char_color.green == 0 && char_color.blue == 0 {
                    char_color = background_color;
                }

                let y_offset = (metrics.ymin.abs() as usize).saturating_add(
                    if (font.font_size as usize) < metrics.height {
                        0
                    } else {
                        (font.font_size as usize).saturating_sub(metrics.height)
                    },
                );

                let final_y = current_y.saturating_add(y_offset);

                // Only draw if within bounds
                if current_x < self.width && final_y < self.height {
                    self.put_pixel(current_x, final_y, char_color);
                }
                current_x += 1;
            }
            current_y += 1;
            current_x = x;
        }
    }

    pub fn draw_bitmap(&mut self, bitmap: Vec<Vec<u32>>) {
        for y in 0..bitmap.len() {
            for x in 0..bitmap[y].len() {
                let buf_pos = self.calc_buf_pos(x, y);
                self.buffer[buf_pos] = bitmap[y][x]
            }
        }
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        let buf_pos = self.calc_buf_pos(x, y);
        if self.buffer.len() > buf_pos {
            self.buffer[buf_pos] = color.to_hex_rgb();
        }
    }

    pub fn put_pixel_a(&mut self, x: usize, y: usize, color: Color) {
        let buf_pos = self.calc_buf_pos(x, y);
        if self.buffer.len() > buf_pos {
            self.buffer[buf_pos] = color.to_hex_rgba();
        }
    }

    pub fn clear(&mut self, color: Color) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn draw_performance_chart(
        &mut self,
        tracker: &PerformanceTracker,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        font: &Font,
    ) {
        // 1. Draw the "Glass" background
        let bg_color = Color::from_rgba(10, 10, 10, 50);
        self.draw_rect_alpha(x, y, width, height, bg_color);

        let max_fps = 120.0;
        let x_step = width as f32 / (tracker.max_samples as f32);

        // Define our three colors
        let colors = [
            Color::from_rgb(0, 150, 255), // Server: Sky Blue
            Color::from_rgb(255, 200, 0), // Received: Gold
            Color::from_rgb(0, 255, 100), // Render: Neon Green
        ];

        let histories = [
            &tracker.server_history,
            &tracker.received_history,
            &tracker.render_history,
        ];

        // 2. Draw each line
        for (h_idx, history) in histories.iter().enumerate() {
            if history.len() < 2 {
                continue;
            }
            let color = colors[h_idx];

            for i in 0..history.len() - 1 {
                let x1 = x + (i as f32 * x_step) as usize;
                let x2 = x + ((i + 1) as f32 * x_step) as usize;

                let y1 = (y + height) - ((history[i] / max_fps) * height as f32) as usize;
                let y2 = (y + height) - ((history[i + 1] / max_fps) * height as f32) as usize;

                // Ensure we don't draw outside the chart box
                if y1 >= y && y2 >= y {
                    self.draw_line_safe(x1, y1, x2, y2, color);
                }
            }
        }

        // 3. Draw a tiny Legend with Fira Code
/*         self.draw_text("SRV", x + 5, y + 5, colors[0], font);
        self.draw_text("NET", x + 45, y + 5, colors[1], font);
        self.draw_text("GPU", x + 85, y + 5, colors[2], font); */
    }

    // A robust line drawer (Bresenham's light)
    fn draw_line_safe(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: Color) {
        let dx = (x1 as i32 - x0 as i32).abs();
        let dy = (y1 as i32 - y0 as i32).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx - dy;

        let (mut x, mut y) = (x0 as i32, y0 as i32);
        loop {
            self.put_pixel(x as usize, y as usize, color);
            if x == x1 as i32 && y == y1 as i32 {
                break;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }
}
