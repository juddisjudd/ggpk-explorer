use eframe::egui;

pub struct HexViewer;

/// One row of the view: the offset, the bytes in hex, then the bytes as text.
fn row_text(data: &[u8], row: usize, per_row: usize) -> String {
    let start = row * per_row;
    let chunk = &data[start..(start + per_row).min(data.len())];
    let mut text = format!("{:08X}: ", start);
    for byte in chunk {
        text.push_str(&format!("{:02X} ", byte));
    }
    text.push_str(&" ".repeat((per_row - chunk.len()) * 3));
    text.push_str(" | ");
    text.extend(chunk.iter().map(|&b| if (32..=126).contains(&b) { b as char } else { '.' }));
    text
}

impl HexViewer {
    /// Shows `data` as rows of hex. Only the rows inside the clip rectangle are laid out, so a file of any size costs one screenful per frame.
    pub fn show(ui: &mut egui::Ui, data: &[u8]) {
        ui.label(format!("Raw Data ({} bytes)", data.len()));
        let font_id = egui::FontId::monospace(12.0);
        let (char_width, row_height) = ui.fonts(|f| (f.glyph_width(&font_id, '0'), f.row_height(&font_id)));
        // A row is "00000000: " + "XX " per byte + " | " + one character per byte: 13 + 4 per byte.
        let fit = ((ui.available_width().max(300.0) / char_width - 13.0) / 4.0).max(0.0) as usize;
        let per_row = ((fit / 8) * 8).max(16);
        let rows = data.len().div_ceil(per_row);

        let (rect, _) = ui.allocate_exact_size(egui::vec2(char_width * (13 + 4 * per_row) as f32, rows as f32 * row_height), egui::Sense::hover());
        let clip = ui.clip_rect();
        let first = ((clip.top() - rect.top()) / row_height).floor().max(0.0) as usize;
        let last = (((clip.bottom() - rect.top()) / row_height).ceil().max(0.0) as usize).min(rows);
        for row in first..last {
            let top_left = egui::pos2(rect.left(), rect.top() + row as f32 * row_height);
            let line = egui::Rect::from_min_size(top_left, egui::vec2(rect.width(), row_height));
            ui.put(line, egui::Label::new(egui::RichText::new(row_text(data, row, per_row)).font(font_id.clone())).extend());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_its_offset_its_bytes_and_their_text() {
        let data: Vec<u8> = (0..40u8).map(|i| b'A' + i % 26).collect();
        assert_eq!(row_text(&data, 0, 16), "00000000: 41 42 43 44 45 46 47 48 49 4A 4B 4C 4D 4E 4F 50  | ABCDEFGHIJKLMNOP");
        assert_eq!(row_text(&data, 2, 16), format!("00000020: 47 48 49 4A 4B 4C 4D 4E {} | GHIJKLMN", " ".repeat(24)));
        assert!(row_text(&[0, 255, 10], 0, 16).ends_with(" | ..."));
    }

    #[test]
    fn a_large_file_lays_out_only_the_rows_on_screen() {
        let data = vec![7u8; 10 * 1024 * 1024];
        let ctx = egui::Context::default();
        let started = std::time::Instant::now();
        let mut texts = 0;
        for _ in 0..3 {
            let screen = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))), ..Default::default() };
            let output = ctx.run(screen, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| HexViewer::show(ui, &data));
                });
            });
            fn count(shape: &egui::Shape) -> usize {
                match shape {
                    egui::Shape::Text(_) => 1,
                    egui::Shape::Vec(shapes) => shapes.iter().map(count).sum(),
                    _ => 0,
                }
            }
            texts = output.shapes.iter().map(|s| count(&s.shape)).sum();
        }
        assert!((2..80).contains(&texts), "{} pieces of text for a ten-megabyte file on a 720-pixel screen", texts);
        assert!(started.elapsed().as_secs() < 2, "three frames took {:?}", started.elapsed());
    }
}
