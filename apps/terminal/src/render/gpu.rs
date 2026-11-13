//! The OpenGL half: the atlas texture, one instance buffer each for backgrounds, glyphs and the
//! lines over them (underlines, strike-through, a thin cursor), and three instanced draws.
//!
//! Each buffer has a fixed slot range per grid row, so a changed row is one `buffer_sub_data`
//! and an unchanged one is never sent again. Unused slots are zero-sized and draw nothing.
//! Positions are whole pixels from the grid's top-left; the viewport is the grid's rectangle.

use super::atlas::ATLAS;
use super::color::COLOR_ATLAS;
use super::{GlyphInst, RectInst, Update};
use glow::HasContext;

/// Slots per row in the overlay buffer, per column: an underline run and a strike-through run
/// can both start at any column.
pub const OVER_PER_COL: usize = 2;

const RECT_VS: &str = r#"
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec4 a_color;
uniform vec2 u_view;
out vec4 v_color;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    vec2 px = a_pos + corner * a_size;
    gl_Position = vec4(px.x / u_view.x * 2.0 - 1.0, 1.0 - px.y / u_view.y * 2.0, 0.0, 1.0);
    v_color = a_color;
}
"#;

const RECT_FS: &str = r#"
in vec4 v_color;
out vec4 o_color;
void main() {
    o_color = vec4(v_color.rgb * v_color.a, v_color.a);
}
"#;

const GLYPH_VS: &str = r#"
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec2 a_size;
layout(location = 3) in vec4 a_color;
uniform vec2 u_view;
uniform float u_atlas;
out vec2 v_uv;
out vec4 v_color;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    vec2 px = a_pos + corner * a_size;
    gl_Position = vec4(px.x / u_view.x * 2.0 - 1.0, 1.0 - px.y / u_view.y * 2.0, 0.0, 1.0);
    v_uv = (a_uv + corner * a_size) / u_atlas;
    v_color = a_color;
}
"#;

/// Coverage from DirectWrite is linear; blended as it is in sRGB, light text on a dark ground
/// looks thin and dark text on a light one heavy. A small lift by the text's own lightness evens
/// them out, as DirectWrite's own "enhanced contrast" does.
const GLYPH_FS: &str = r#"
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_tex;
out vec4 o_color;
void main() {
    float cov = texture(u_tex, v_uv).r;
    float light = dot(v_color.rgb, vec3(0.30, 0.59, 0.11));
    cov = pow(cov, mix(1.0, 0.8, light));
    float a = cov * v_color.a;
    o_color = vec4(v_color.rgb * a, a);
}
"#;

/// Colour glyphs: the page holds premultiplied colour (BGRA, uploaded as such), drawn as it is,
/// faded by the instance's alpha.
const COLOR_FS: &str = r#"
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_tex;
out vec4 o_color;
void main() {
    o_color = texture(u_tex, v_uv) * v_color.a;
}
"#;

struct Buffers {
    vao: glow::VertexArray,
    buffer: glow::Buffer,
}

pub struct Gpu {
    rect: glow::Program,
    glyph: glow::Program,
    atlas: glow::Texture,
    bg: Buffers,
    glyphs: Buffers,
    over: Buffers,
    /// Colour glyphs (emoji): their program, page and buffer, made when the first one shows.
    color: Option<(glow::Program, glow::Texture)>,
    emoji: Buffers,
    any_emoji: bool,
    cols: usize,
    rows: usize,
    /// The pages' sides (greyscale, colour), as the renderer made them.
    atlas_side: i32,
    color_side: i32,
    max_texture: i32,
}

impl Gpu {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let rect = program(gl, RECT_VS, RECT_FS)?;
            let glyph = program(gl, GLYPH_VS, GLYPH_FS)?;
            let atlas = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas));
            mono_storage(gl, ATLAS);
            // Glyphs land on whole pixels, so nearest sampling is exact.
            for (p, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
                (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            Ok(Self {
                rect,
                glyph,
                atlas,
                bg: rect_buffers(gl)?,
                glyphs: glyph_buffers(gl)?,
                over: rect_buffers(gl)?,
                color: None,
                emoji: glyph_buffers(gl)?,
                any_emoji: false,
                cols: 0,
                rows: 0,
                atlas_side: ATLAS,
                color_side: COLOR_ATLAS,
                max_texture: gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE),
            })
        }
    }

    /// The largest texture side this GPU takes.
    pub fn max_texture(&self) -> i32 {
        self.max_texture
    }

    /// Take in what changed since the last frame: a new grid size, new glyphs, changed rows.
    pub fn apply(&mut self, gl: &glow::Context, update: &mut Update) {
        unsafe {
            if let Some((cols, rows)) = update.resize.take() {
                self.cols = cols;
                self.rows = rows;
                let cells = cols * rows;
                alloc(gl, &self.bg, cells * size_of::<RectInst>());
                alloc(gl, &self.glyphs, cells * size_of::<GlyphInst>());
                alloc(gl, &self.over, cells * OVER_PER_COL * size_of::<RectInst>());
                alloc(gl, &self.emoji, cells * size_of::<GlyphInst>());
            }
            // New pages (another font size): the textures take their size, emptied.
            if let Some((mono, color)) = update.pages.take() {
                if mono != self.atlas_side {
                    gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas));
                    mono_storage(gl, mono);
                    gl.bind_texture(glow::TEXTURE_2D, None);
                    self.atlas_side = mono;
                }
                if color != self.color_side {
                    if let Some((_, tex)) = self.color {
                        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                        color_storage(gl, color);
                        gl.bind_texture(glow::TEXTURE_2D, None);
                    }
                    self.color_side = color;
                }
            }
            if !update.color_glyphs.is_empty() {
                if self.color.is_none() {
                    match color_page(gl, self.color_side) {
                        Ok(page) => self.color = Some(page),
                        Err(e) => eprintln!("terminal: no colour glyphs: {e}"),
                    }
                }
                if let Some((_, tex)) = self.color {
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                    for u in update.color_glyphs.drain(..) {
                        gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, u.x, u.y, u.w, u.h, glow::BGRA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&u.pixels)));
                    }
                    gl.bind_texture(glow::TEXTURE_2D, None);
                }
                update.color_glyphs.clear();
            }
            self.any_emoji = update.any_emoji;
            if !update.glyphs.is_empty() {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                for u in update.glyphs.drain(..) {
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        u.x,
                        u.y,
                        u.w,
                        u.h,
                        glow::RED,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&u.pixels)),
                    );
                }
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
            let cols = self.cols;
            for row in update.rows.drain(..) {
                if row.index >= self.rows || row.glyphs.len() != cols {
                    continue;
                }
                put(gl, &self.bg, row.index * cols * size_of::<RectInst>(), &row.bg);
                put(gl, &self.glyphs, row.index * cols * size_of::<GlyphInst>(), &row.glyphs);
                put(gl, &self.over, row.index * cols * OVER_PER_COL * size_of::<RectInst>(), &row.over);
                if let Some(emoji) = &row.emoji {
                    put(gl, &self.emoji, row.index * cols * size_of::<GlyphInst>(), emoji);
                }
            }
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    /// Draw the grid into the current viewport, `view` pixels in size.
    pub fn draw(&self, gl: &glow::Context, view: [f32; 2]) {
        let cells = (self.cols * self.rows) as i32;
        if cells == 0 {
            return;
        }
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            // Everything is drawn premultiplied, the way egui draws and DWM composes.
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::DEPTH_TEST);

            gl.use_program(Some(self.rect));
            gl.uniform_2_f32(gl.get_uniform_location(self.rect, "u_view").as_ref(), view[0], view[1]);
            gl.bind_vertex_array(Some(self.bg.vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, cells);

            gl.use_program(Some(self.glyph));
            gl.uniform_2_f32(gl.get_uniform_location(self.glyph, "u_view").as_ref(), view[0], view[1]);
            gl.uniform_1_f32(gl.get_uniform_location(self.glyph, "u_atlas").as_ref(), self.atlas_side as f32);
            gl.uniform_1_i32(gl.get_uniform_location(self.glyph, "u_tex").as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas));
            gl.bind_vertex_array(Some(self.glyphs.vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, cells);

            // Colour glyphs, only while some row on screen has one.
            if let (true, Some((program, tex))) = (self.any_emoji, self.color) {
                gl.use_program(Some(program));
                gl.uniform_2_f32(gl.get_uniform_location(program, "u_view").as_ref(), view[0], view[1]);
                gl.uniform_1_f32(gl.get_uniform_location(program, "u_atlas").as_ref(), self.color_side as f32);
                gl.uniform_1_i32(gl.get_uniform_location(program, "u_tex").as_ref(), 0);
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.bind_vertex_array(Some(self.emoji.vao));
                gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, cells);
            }

            gl.use_program(Some(self.rect));
            gl.bind_vertex_array(Some(self.over.vao));
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, cells * OVER_PER_COL as i32);

            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.rect);
            gl.delete_program(self.glyph);
            gl.delete_texture(self.atlas);
            if let Some((program, tex)) = self.color {
                gl.delete_program(program);
                gl.delete_texture(tex);
            }
            for b in [&self.bg, &self.glyphs, &self.over, &self.emoji] {
                gl.delete_vertex_array(b.vao);
                gl.delete_buffer(b.buffer);
            }
        }
    }
}

/// (Re)make the bound texture's storage as a greyscale page of `side`.
unsafe fn mono_storage(gl: &glow::Context, side: i32) {
    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::R8 as i32, side, side, 0, glow::RED, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
}

/// (Re)make the bound texture's storage as a colour page of `side`.
unsafe fn color_storage(gl: &glow::Context, side: i32) {
    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, side, side, 0, glow::BGRA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
}

/// The colour page (of `side`) and the program that draws from it.
unsafe fn color_page(gl: &glow::Context, side: i32) -> Result<(glow::Program, glow::Texture), String> {
    let program = program(gl, GLYPH_VS, COLOR_FS)?;
    let tex = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    color_storage(gl, side);
    for (p, v) in [
        (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
        (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
        (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
        (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
    ] {
        gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
    }
    gl.bind_texture(glow::TEXTURE_2D, None);
    Ok((program, tex))
}

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let shader = gl.create_shader(kind)?;
        gl.shader_source(shader, &format!("#version 330 core\n{src}"));
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(gl.get_shader_info_log(shader));
        }
        gl.attach_shader(program, shader);
        shaders.push(shader);
    }
    gl.link_program(program);
    for s in shaders {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    if !gl.get_program_link_status(program) {
        return Err(gl.get_program_info_log(program));
    }
    Ok(program)
}

/// One instanced attribute: `count` values of `kind` at `offset` in a `stride`-byte instance.
unsafe fn attr(gl: &glow::Context, index: u32, count: i32, kind: u32, normalized: bool, stride: usize, offset: usize) {
    gl.enable_vertex_attrib_array(index);
    gl.vertex_attrib_pointer_f32(index, count, kind, normalized, stride as i32, offset as i32);
    gl.vertex_attrib_divisor(index, 1);
}

unsafe fn rect_buffers(gl: &glow::Context) -> Result<Buffers, String> {
    let vao = gl.create_vertex_array()?;
    let buffer = gl.create_buffer()?;
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
    let s = size_of::<RectInst>();
    attr(gl, 0, 2, glow::SHORT, false, s, 0);
    attr(gl, 1, 2, glow::UNSIGNED_SHORT, false, s, 4);
    attr(gl, 2, 4, glow::UNSIGNED_BYTE, true, s, 8);
    gl.bind_vertex_array(None);
    gl.bind_buffer(glow::ARRAY_BUFFER, None);
    Ok(Buffers { vao, buffer })
}

unsafe fn glyph_buffers(gl: &glow::Context) -> Result<Buffers, String> {
    let vao = gl.create_vertex_array()?;
    let buffer = gl.create_buffer()?;
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
    let s = size_of::<GlyphInst>();
    attr(gl, 0, 2, glow::SHORT, false, s, 0);
    attr(gl, 1, 2, glow::UNSIGNED_SHORT, false, s, 4);
    attr(gl, 2, 2, glow::UNSIGNED_SHORT, false, s, 8);
    attr(gl, 3, 4, glow::UNSIGNED_BYTE, true, s, 12);
    gl.bind_vertex_array(None);
    gl.bind_buffer(glow::ARRAY_BUFFER, None);
    Ok(Buffers { vao, buffer })
}

/// Give a buffer `bytes` of zeros (zero-sized instances draw nothing).
unsafe fn alloc(gl: &glow::Context, b: &Buffers, bytes: usize) {
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.buffer));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &vec![0u8; bytes], glow::DYNAMIC_DRAW);
}

unsafe fn put<T: Copy>(gl: &glow::Context, b: &Buffers, offset: usize, items: &[T]) {
    let bytes = std::slice::from_raw_parts(items.as_ptr().cast::<u8>(), std::mem::size_of_val(items));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.buffer));
    gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, offset as i32, bytes);
}
