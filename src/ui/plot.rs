//! Small painter-drawn line plots shared by the curve, JSON and timeline viewers.

use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

pub struct Series<'a> {
    pub points: &'a [(f32, f32)],
    pub color: Color32,
}

fn fmt_num(v: f32) -> String {
    if v.abs() >= 100.0 || v.fract() == 0.0 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{:.3}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Draws one or more series in a `size` box with min/max labels. Points are sorted by x by the caller.
pub fn draw(ui: &mut egui::Ui, series: &[Series<'_>], size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    let bg = if dark { Color32::from_rgb(28, 28, 34) } else { Color32::from_rgb(246, 246, 250) };
    let grid = if dark { Color32::from_rgb(52, 52, 60) } else { Color32::from_rgb(214, 214, 222) };
    let text = if dark { Color32::from_rgb(150, 150, 160) } else { Color32::from_rgb(110, 110, 120) };
    painter.rect_filled(rect, 3.0, bg);

    let all: Vec<(f32, f32)> = series.iter().flat_map(|s| s.points.iter().copied()).collect();
    if all.len() < 2 {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "no data", egui::FontId::proportional(11.0), text);
        return resp;
    }
    let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut y0, mut y1) = (f32::INFINITY, f32::NEG_INFINITY);
    for (x, y) in &all {
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    if (x1 - x0).abs() < 1e-6 {
        x1 = x0 + 1.0;
    }
    if (y1 - y0).abs() < 1e-6 {
        y0 -= 0.5;
        y1 += 0.5;
    }
    let inner = rect.shrink2(Vec2::new(6.0, 12.0));
    let map = |x: f32, y: f32| Pos2::new(inner.left() + (x - x0) / (x1 - x0) * inner.width(), inner.bottom() - (y - y0) / (y1 - y0) * inner.height());

    for k in 0..=2 {
        let y = inner.top() + inner.height() * k as f32 / 2.0;
        painter.line_segment([Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)], Stroke::new(1.0_f32, grid));
    }
    if y0 < 0.0 && y1 > 0.0 {
        let zero = map(x0, 0.0).y;
        painter.line_segment([Pos2::new(inner.left(), zero), Pos2::new(inner.right(), zero)], Stroke::new(1.0_f32, text));
    }
    for s in series {
        let pts: Vec<Pos2> = s.points.iter().map(|(x, y)| map(*x, *y)).collect();
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.5_f32, s.color)));
        if pts.len() <= 16 {
            for p in pts {
                painter.circle_filled(p, 2.0, s.color);
            }
        }
    }
    let font = egui::FontId::proportional(9.5);
    painter.text(Pos2::new(rect.left() + 3.0, rect.top() + 1.0), egui::Align2::LEFT_TOP, fmt_num(y1), font.clone(), text);
    painter.text(Pos2::new(rect.left() + 3.0, rect.bottom() - 1.0), egui::Align2::LEFT_BOTTOM, fmt_num(y0), font.clone(), text);
    painter.text(Pos2::new(rect.right() - 3.0, rect.bottom() - 1.0), egui::Align2::RIGHT_BOTTOM, format!("{}→{}", fmt_num(x0), fmt_num(x1)), font, text);

    if let Some(hover) = resp.hover_pos() {
        let fx = x0 + (hover.x - inner.left()) / inner.width() * (x1 - x0);
        let mut best: Option<(f32, f32)> = None;
        for (x, y) in &all {
            if best.map(|(bx, _)| (x - fx).abs() < (bx - fx).abs()).unwrap_or(true) {
                best = Some((*x, *y));
            }
        }
        if let Some((bx, by)) = best {
            let p = map(bx, by);
            painter.circle_stroke(p, 3.5, Stroke::new(1.0_f32, text));
            resp.clone().on_hover_text(format!("x {}  y {}", fmt_num(bx), fmt_num(by)));
        }
    }
    resp
}

pub fn series_color(i: usize, dark: bool) -> Color32 {
    const DARK: [Color32; 6] = [
        Color32::from_rgb(97, 175, 239),
        Color32::from_rgb(152, 195, 121),
        Color32::from_rgb(224, 108, 117),
        Color32::from_rgb(229, 192, 123),
        Color32::from_rgb(198, 120, 221),
        Color32::from_rgb(86, 182, 194),
    ];
    const LIGHT: [Color32; 6] = [
        Color32::from_rgb(9, 79, 172),
        Color32::from_rgb(3, 117, 43),
        Color32::from_rgb(180, 40, 50),
        Color32::from_rgb(170, 110, 0),
        Color32::from_rgb(120, 40, 160),
        Color32::from_rgb(13, 116, 124),
    ];
    if dark { DARK[i % DARK.len()] } else { LIGHT[i % LIGHT.len()] }
}
