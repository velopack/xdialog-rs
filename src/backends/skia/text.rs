use fontdue::Font;
use tiny_skia::PixmapMut;

pub struct TextLine {
    pub text: String,
    #[allow(dead_code)]
    pub width: f32,
}

pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub total_width: f32,
    pub total_height: f32,
    pub line_height: f32,
}

/// Measure the width of a single line of text (no wrapping).
pub fn measure_text_width(text: &str, font: &Font, size: f32) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, size).advance_width)
        .sum()
}

/// Measure the height of a single line.
pub fn measure_line_height(font: &Font, size: f32) -> f32 {
    let metrics = font.horizontal_line_metrics(size).unwrap();
    metrics.new_line_size
}

/// Layout text with word wrapping at max_width. Returns line info for rendering.
pub fn layout_text(text: &str, font: &Font, size: f32, max_width: f32) -> TextLayout {
    let line_height = measure_line_height(font, size);
    let mut lines = Vec::new();
    let mut total_width: f32 = 0.0;

    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(TextLine {
                text: String::new(),
                width: 0.0,
            });
            continue;
        }

        let mut current_line = String::new();
        let mut current_width: f32 = 0.0;

        for word in paragraph.split_whitespace() {
            let word_width = measure_text_width(word, font, size);
            let space_width = if current_line.is_empty() {
                0.0
            } else {
                font.metrics(' ', size).advance_width
            };

            if !current_line.is_empty() && current_width + space_width + word_width > max_width {
                // Wrap to next line
                total_width = total_width.max(current_width);
                lines.push(TextLine {
                    text: current_line,
                    width: current_width,
                });
                current_line = word.to_string();
                current_width = word_width;
            } else {
                if !current_line.is_empty() {
                    current_line.push(' ');
                    current_width += space_width;
                }
                current_line.push_str(word);
                current_width += word_width;
            }
        }

        // Push remaining text
        total_width = total_width.max(current_width);
        lines.push(TextLine {
            text: current_line,
            width: current_width,
        });
    }

    if lines.is_empty() {
        lines.push(TextLine {
            text: String::new(),
            width: 0.0,
        });
    }

    let total_height = lines.len() as f32 * line_height;

    TextLayout {
        lines,
        total_width,
        total_height,
        line_height,
    }
}

/// Render text layout onto a pixmap at the given position.
pub fn render_text(
    pixmap: &mut PixmapMut,
    layout: &TextLayout,
    font: &Font,
    size: f32,
    color: (u8, u8, u8),
    x: f32,
    y: f32,
) {
    let metrics = font.horizontal_line_metrics(size).unwrap();
    let ascent = metrics.ascent;

    for (i, line) in layout.lines.iter().enumerate() {
        let line_y = y + i as f32 * layout.line_height + ascent;
        render_line(pixmap, &line.text, font, size, color, x, line_y);
    }
}

/// Render a single line of text at the given baseline position.
fn render_line(
    pixmap: &mut PixmapMut,
    text: &str,
    font: &Font,
    size: f32,
    color: (u8, u8, u8),
    x: f32,
    baseline_y: f32,
) {
    let mut cursor_x = x;
    let (cr, cg, cb) = color;

    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);

        if !bitmap.is_empty() && metrics.width > 0 && metrics.height > 0 {
            let glyph_x = cursor_x as i32 + metrics.xmin;
            let glyph_y = baseline_y as i32 - metrics.height as i32 - metrics.ymin;

            let pm_width = pixmap.width() as i32;
            let pm_height = pixmap.height() as i32;
            let data = pixmap.data_mut();

            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let px = glyph_x + col as i32;
                    let py = glyph_y + row as i32;

                    if px >= 0 && px < pm_width && py >= 0 && py < pm_height {
                        let alpha = bitmap[row * metrics.width + col];
                        if alpha > 0 {
                            let idx = (py as usize * pm_width as usize + px as usize) * 4;
                            if alpha == 255 {
                                data[idx] = cr;
                                data[idx + 1] = cg;
                                data[idx + 2] = cb;
                                data[idx + 3] = 255;
                            } else {
                                // Alpha blend
                                let a = alpha as f32 / 255.0;
                                let inv_a = 1.0 - a;
                                data[idx] = (cr as f32 * a + data[idx] as f32 * inv_a) as u8;
                                data[idx + 1] =
                                    (cg as f32 * a + data[idx + 1] as f32 * inv_a) as u8;
                                data[idx + 2] =
                                    (cb as f32 * a + data[idx + 2] as f32 * inv_a) as u8;
                                data[idx + 3] =
                                    (255.0f32.min(data[idx + 3] as f32 + alpha as f32)) as u8;
                            }
                        }
                    }
                }
            }
        }

        cursor_x += metrics.advance_width;
    }
}
