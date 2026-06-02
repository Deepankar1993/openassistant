// src/canvas/mod.rs
pub mod renderer;

/// Live Canvas - Agent-driven visual workspace (OpenClaw-style)
#[derive(Debug, Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub elements: Vec<CanvasElement>,
}

#[derive(Debug, Clone)]
pub enum CanvasElement {
    Text { x: u32, y: u32, content: String },
    Image { x: u32, y: u32, path: String },
    Chart { x: u32, y: u32, data: Vec<f64> },
    Html { content: String },
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            elements: Vec::new(),
        }
    }

    pub fn add_text(&mut self, x: u32, y: u32, content: impl Into<String>) {
        self.elements.push(CanvasElement::Text { x, y, content: content.into() });
    }

    pub fn clear(&mut self) {
        self.elements.clear();
    }
}
