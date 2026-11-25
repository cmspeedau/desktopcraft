use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
varying mediump vec2 uv;
varying lowp vec4 color;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
    color = color0 / 255.0;
}"#;

const FRAGMENT: &str = r#"#version 100
varying mediump vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
void main() {
    gl_FragColor = texture2D(Texture, uv) * color;
}"#;

pub(crate) struct Renderer {
    capture: Material,
    present: Material,
}

impl Renderer {
    pub(crate) fn new() -> Option<Self> {
        let blend = |source| BlendState::new(
            Equation::Add, source, BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        );
        let alpha = blend(BlendFactor::One);
        let material = |color_blend| load_material(
            ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT },
            MaterialParams {
                pipeline_params: PipelineParams {
                    color_blend: Some(color_blend),
                    alpha_blend: Some(alpha),
                    ..Default::default()
                },
                ..Default::default()
            },
        ).ok();
        Some(Self {
            // Store premultiplied pixels with source-over alpha, then
            // composite those pixels without applying source alpha twice.
            capture: material(blend(BlendFactor::Value(BlendValue::SourceAlpha)))?,
            present: material(alpha)?,
        })
    }

    pub(crate) fn capture(&self, area: Rect, pixels: (u32, u32), draw: impl FnOnce()) -> RenderTarget {
        let target = render_target(pixels.0, pixels.1);
        target.texture.set_filter(FilterMode::Nearest);
        let mut camera = Camera2D::from_display_rect(area);
        camera.render_target = Some(target.clone());
        push_camera_state();
        set_camera(&camera);
        clear_background(BLANK);
        gl_use_material(&self.capture);
        draw();
        gl_use_default_material();
        pop_camera_state();
        // Miniquad deletes the attached texture when the render pass is
        // collected. The caller must own the complete target while it draws.
        target
    }

    pub(crate) fn draw(&self, snapshot: &RenderTarget, at: Vec2, size: Vec2) {
        gl_use_material(&self.present);
        draw_texture_ex(&snapshot.texture, at.x, at.y, WHITE, DrawTextureParams {
            dest_size: Some(size),
            flip_y: true,
            ..Default::default()
        });
        gl_use_default_material();
    }
}

/// The windowed self-test holds this snapshot across frame cleanup.
pub(crate) struct GraphicsProbe {
    renderer: Renderer,
    source: RenderTarget,
}

impl GraphicsProbe {
    pub(crate) fn new() -> Option<Self> {
        let renderer = Renderer::new()?;
        let source = renderer.capture(Rect::new(0., 0., 12., 12.), (12, 12), || {
            draw_rectangle(0., 0., 5., 5., Color::new(1., 0., 0., 0.5));
            draw_rectangle(7., 7., 5., 5., GREEN);
        });
        Some(Self { renderer, source })
    }

    pub(crate) fn verify(self) -> bool {
        let result = self.renderer.capture(Rect::new(0., 0., 16., 16.), (16, 16), || {
            self.renderer.draw(&self.source, vec2(2., 1.), vec2(12., 12.));
        }).texture.get_texture_data();
        // OpenGL readback starts at the bottom scanline.
        let red = result.get_pixel(4, 12);
        let green = result.get_pixel(11, 5);
        let blank = result.get_pixel(13, 12);
        (red.r - 0.5).abs() < 0.02 && (red.a - 0.5).abs() < 0.02
            && green.g > 0.75 && green.a > 0.99 && blank.a < 0.01
    }
}
