// src/canvas/renderer.rs
use super::Canvas;

/// Render canvas to HTML for display
pub fn render_html(canvas: &Canvas) -> String {
    let mut html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<style>
body {{ margin: 0; padding: 20px; background: #1a1a2e; color: #eee; font-family: monospace; }}
.element {{ position: absolute; }}
</style>
</head>
<body>
<div id="canvas" style="width:{}px; height:{}px; position: relative;">
"#,
        canvas.width, canvas.height
    );

    for elem in &canvas.elements {
        match elem {
            super::CanvasElement::Text { x, y, content } => {
                html.push_str(&format!(
                    r#"<div class="element" style="left:{}px; top:{}px;">{}</div>"#,
                    x, y, content
                ));
            }
            _ => {}
        }
    }

    html.push_str("</div></body></html>");
    html
}
