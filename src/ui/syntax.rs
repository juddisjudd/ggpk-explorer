use eframe::egui;

#[allow(dead_code)]
pub struct Theme {
    pub display_name: String,
    pub background: egui::Color32,
    pub cursor: egui::Color32,
    pub selection: egui::Color32,
    pub normal: egui::Color32,
    pub keyword: egui::Color32,
    pub literal: egui::Color32,
    pub comment: egui::Color32,
    pub type_name: egui::Color32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            display_name: "Dark".into(),
            background: egui::Color32::from_rgb(30, 30, 30),
            cursor: egui::Color32::WHITE,
            selection: egui::Color32::from_rgba_premultiplied(60, 100, 160, 60),
            normal: egui::Color32::from_gray(200),
            keyword: egui::Color32::from_rgb(86, 156, 214), // VSCode blue
            literal: egui::Color32::from_rgb(181, 206, 168), // Greenish for strings/numbers
            comment: egui::Color32::from_rgb(106, 153, 85), // Green
            type_name: egui::Color32::from_rgb(78, 201, 176), // Teal
        }
    }

    pub fn light() -> Self {
        Self {
            display_name: "Light".into(),
            background: egui::Color32::from_rgb(250, 250, 252),
            cursor: egui::Color32::BLACK,
            selection: egui::Color32::from_rgba_premultiplied(200, 220, 240, 120),
            normal: egui::Color32::from_rgb(24, 24, 28),
            keyword: egui::Color32::from_rgb(9, 79, 172), // Blue
            literal: egui::Color32::from_rgb(3, 117, 43), // Greenish
            comment: egui::Color32::from_rgb(100, 100, 110), // Grayish green
            type_name: egui::Color32::from_rgb(112, 26, 173), // Purple
        }
    }
}

pub fn highlight(ctx: &egui::Context, theme: &Theme, text: &str, language: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    if language == "hlsl" {
        highlight_hlsl(ctx, theme, text, &mut job);
    } else {
        // Plain text fallback
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::monospace(14.0),
                color: theme.normal,
                ..Default::default()
            },
        );
    }

    job
}

fn highlight_hlsl(_ctx: &egui::Context, theme: &Theme, text: &str, job: &mut egui::text::LayoutJob) {
    let font_id = egui::FontId::monospace(14.0);
    
    // Trivial parser: scan line by line or use a simple state machine?
    // Since we don't need perfect parsing, we can just split by delimiters and color.
    // However, handling strings and comments requires a bit of state.

    let mut chars = text.char_indices().peekable();
    let mut start = 0;
    
    // Simple state
    let mut is_comment_line = false;
    let mut is_block_comment = false;
    let mut is_string = false;
    
    // Keywords
    let keywords = [
        "float", "float2", "float3", "float4", 
        "int", "bool", "void", "struct", "cbuffer", 
        "Texture2D", "SamplerState", "TextureCube",
        "return", "if", "else", "for", "while", "do",
        "technique", "pass", "VertexShader", "PixelShader",
        "register", "in", "out", "inout", "static", "const"
    ];

    while let Some((idx, c)) = chars.next() {
        if is_comment_line {
            if c == '\n' {
                is_comment_line = false;
                append(job, &text[start..idx+1], theme.comment, &font_id);
                start = idx + 1;
            }
        } else if is_block_comment {
             if c == '*' {
                 if let Some((_, '/')) = chars.peek() {
                     chars.next(); // consume '/'
                     is_block_comment = false;
                     append(job, &text[start..idx+2], theme.comment, &font_id); // include */
                     start = idx + 2;
                 }
             }
        } else if is_string {
            if c == '"' { // Simplified main string
                 is_string = false;
                 append(job, &text[start..idx+1], theme.literal, &font_id);
                 start = idx + 1;
            }
        } else {
            // Check for start of special blocks
            if c == '/' {
                if let Some((_, '/')) = chars.peek() {
                    // Start of line comment
                    // Flush previous text
                    if idx > start {
                        highlight_code_span(job, &text[start..idx], theme, &keywords, &font_id);
                    }
                    start = idx;
                    is_comment_line = true;
                    chars.next(); // consume 2nd /
                } else if let Some((_, '*')) = chars.peek() {
                    // Start of block comment
                    if idx > start {
                        highlight_code_span(job, &text[start..idx], theme, &keywords, &font_id);
                    }
                    start = idx;
                    is_block_comment = true;
                    chars.next(); 
                }
            } else if c == '"' {
                if idx > start {
                     highlight_code_span(job, &text[start..idx], theme, &keywords, &font_id);
                }
                start = idx;
                is_string = true;
            }
            
            // Check newline to flush
            if c == '\n' {
                 if idx >= start {
                      highlight_code_span(job, &text[start..idx+1], theme, &keywords, &font_id);
                 }
                 start = idx + 1;
            }
        }
    }
    
    // Flush remaining
    if start < text.len() {
        if is_comment_line || is_block_comment {
             append(job, &text[start..], theme.comment, &font_id);
        } else if is_string {
             append(job, &text[start..], theme.literal, &font_id);
        } else {
             highlight_code_span(job, &text[start..], theme, &keywords, &font_id);
        }
    }
}

fn append(job: &mut egui::text::LayoutJob, text: &str, color: egui::Color32, font_id: &egui::FontId) {
    job.append(text, 0.0, egui::TextFormat {
        font_id: font_id.clone(),
        color,
        ..Default::default()
    });
}

fn highlight_code_span(job: &mut egui::text::LayoutJob, text: &str, theme: &Theme, keywords: &[&str], font_id: &egui::FontId) {
    // Split by non-alphanumeric to find words, but preserve delimiters
    let mut start = 0;
    
    // We scan indices to find boundary of words
    for (idx, c) in text.char_indices() {
        if !c.is_alphanumeric() && c != '_' {
             if idx > start {
                 let word = &text[start..idx];
                 let color = if keywords.contains(&word) {
                     theme.keyword
                 } else if word.chars().next().unwrap().is_uppercase() {
                     // Heuristic: Types often start with Uppercase in HLSL / Game Engine land
                     theme.type_name
                 } else if word.chars().all(|c| c.is_numeric() || c == '.') {
                     theme.literal
                 } else {
                     theme.normal
                 };
                 append(job, word, color, font_id);
             }
             // Append delimiter
             append(job, &text[idx..idx+c.len_utf8()], theme.normal, font_id);
             start = idx + c.len_utf8();
        }
    }
    
    // Last word found
    if start < text.len() {
        let word = &text[start..];
        let color = if keywords.contains(&word) {
            theme.keyword
        } else if word.chars().next().unwrap_or(' ').is_uppercase() {
             theme.type_name
        } else {
            theme.normal
        };
        append(job, word, color, font_id);
    }
}
