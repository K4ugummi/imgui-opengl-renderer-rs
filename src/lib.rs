use imgui::Context;
use memoffset::offset_of;
use std::ffi::CStr;
use std::mem;
use std::{fmt, ptr};

mod gl {
    #![allow(
        clippy::unreadable_literal,
        clippy::too_many_arguments,
        clippy::unused_unit,
        clippy::upper_case_acronyms,
        clippy::missing_transmute_annotations
    )]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use gl::types::*;

#[derive(Debug)]
pub enum RendererError {
    ShaderCompilation(String),
    ProgramLinking(String),
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendererError::ShaderCompilation(msg) => write!(f, "Shader compilation failed: {msg}"),
            RendererError::ProgramLinking(msg) => write!(f, "Program linking failed: {msg}"),
        }
    }
}

impl std::error::Error for RendererError {}

pub struct Renderer {
    gl: gl::Gl,
    program: GLuint,
    locs: Locs,
    vbo: GLuint,
    ebo: GLuint,
    vao: GLuint,
    font_texture: GLuint,
}

struct Locs {
    texture: GLint,
    proj_mtx: GLint,
    position: GLuint,
    uv: GLuint,
    color: GLuint,
}

impl Renderer {
    pub fn new<F>(imgui: &mut Context, load_fn: F) -> Result<Self, RendererError>
    where
        F: FnMut(&'static str) -> *const ::std::os::raw::c_void,
    {
        let gl = gl::Gl::load_with(load_fn);

        unsafe {
            let glsl_version = b"#version 330\n\0";

            let vert_source = include_str!("shader/default.vs");
            let vert_sources = [
                glsl_version.as_ptr() as *const GLchar,
                vert_source.as_ptr() as *const GLchar,
            ];
            let vert_sources_len = [
                glsl_version.len() as GLint - 1, // exclude null terminator
                vert_source.len() as GLint,
            ];

            let frag_source = include_str!("shader/default.fs");
            let frag_sources = [
                glsl_version.as_ptr() as *const GLchar,
                frag_source.as_ptr() as *const GLchar,
            ];
            let frag_sources_len = [
                glsl_version.len() as GLint - 1, // exclude null terminator
                frag_source.len() as GLint,
            ];

            let program = gl.CreateProgram();
            let vert_shader = gl.CreateShader(gl::VERTEX_SHADER);
            let frag_shader = gl.CreateShader(gl::FRAGMENT_SHADER);
            gl.ShaderSource(
                vert_shader,
                2,
                vert_sources.as_ptr(),
                vert_sources_len.as_ptr(),
            );
            gl.ShaderSource(
                frag_shader,
                2,
                frag_sources.as_ptr(),
                frag_sources_len.as_ptr(),
            );

            gl.CompileShader(vert_shader);
            check_shader_compile(&gl, vert_shader, "vertex")?;

            gl.CompileShader(frag_shader);
            check_shader_compile(&gl, frag_shader, "fragment")?;

            gl.AttachShader(program, vert_shader);
            gl.AttachShader(program, frag_shader);
            gl.LinkProgram(program);
            gl.DeleteShader(vert_shader);
            gl.DeleteShader(frag_shader);

            check_program_link(&gl, program)?;

            let locs = Locs {
                texture: gl.GetUniformLocation(program, c"Texture".as_ptr() as _),
                proj_mtx: gl.GetUniformLocation(program, c"ProjMtx".as_ptr() as _),
                position: gl.GetAttribLocation(program, c"Position".as_ptr() as _) as _,
                uv: gl.GetAttribLocation(program, c"UV".as_ptr() as _) as _,
                color: gl.GetAttribLocation(program, c"Color".as_ptr() as _) as _,
            };

            let vbo = return_param(|x| gl.GenBuffers(1, x));
            let ebo = return_param(|x| gl.GenBuffers(1, x));
            let vao = return_param(|x| gl.GenVertexArrays(1, x));

            let mut current_texture = 0;
            gl.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut current_texture);

            let font_texture = return_param(|x| gl.GenTextures(1, x));
            gl.BindTexture(gl::TEXTURE_2D, font_texture);
            gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as _);
            gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
            gl.PixelStorei(gl::UNPACK_ROW_LENGTH, 0);

            {
                let atlas = imgui.fonts();

                let texture = atlas.build_rgba32_texture();
                gl.TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA as _,
                    texture.width as _,
                    texture.height as _,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    texture.data.as_ptr() as _,
                );

                atlas.tex_id = (font_texture as usize).into();
            }

            gl.BindTexture(gl::TEXTURE_2D, current_texture as _);

            Ok(Self {
                gl,
                program,
                locs,
                vbo,
                ebo,
                vao,
                font_texture,
            })
        }
    }

    pub fn render(&self, ctx: &mut Context) {
        use imgui::{DrawCmd, DrawCmdParams, DrawIdx, DrawVert};

        let gl = &self.gl;

        unsafe {
            let last_active_texture = return_param(|x| gl.GetIntegerv(gl::ACTIVE_TEXTURE, x));
            gl.ActiveTexture(gl::TEXTURE0);
            let last_program = return_param(|x| gl.GetIntegerv(gl::CURRENT_PROGRAM, x));
            let last_texture = return_param(|x| gl.GetIntegerv(gl::TEXTURE_BINDING_2D, x));
            let last_sampler = if gl.BindSampler.is_loaded() {
                return_param(|x| gl.GetIntegerv(gl::SAMPLER_BINDING, x))
            } else {
                0
            };
            let last_array_buffer =
                return_param(|x| gl.GetIntegerv(gl::ARRAY_BUFFER_BINDING, x));
            let last_element_array_buffer =
                return_param(|x| gl.GetIntegerv(gl::ELEMENT_ARRAY_BUFFER_BINDING, x));
            let last_vertex_array =
                return_param(|x| gl.GetIntegerv(gl::VERTEX_ARRAY_BINDING, x));
            let last_polygon_mode = return_param(|x: &mut [GLint; 2]| {
                gl.GetIntegerv(gl::POLYGON_MODE, x.as_mut_ptr())
            });
            let last_viewport = return_param(|x: &mut [GLint; 4]| {
                gl.GetIntegerv(gl::VIEWPORT, x.as_mut_ptr())
            });
            let last_scissor_box = return_param(|x: &mut [GLint; 4]| {
                gl.GetIntegerv(gl::SCISSOR_BOX, x.as_mut_ptr())
            });
            let last_blend_src_rgb = return_param(|x| gl.GetIntegerv(gl::BLEND_SRC_RGB, x));
            let last_blend_dst_rgb = return_param(|x| gl.GetIntegerv(gl::BLEND_DST_RGB, x));
            let last_blend_src_alpha =
                return_param(|x| gl.GetIntegerv(gl::BLEND_SRC_ALPHA, x));
            let last_blend_dst_alpha =
                return_param(|x| gl.GetIntegerv(gl::BLEND_DST_ALPHA, x));
            let last_blend_equation_rgb =
                return_param(|x| gl.GetIntegerv(gl::BLEND_EQUATION_RGB, x));
            let last_blend_equation_alpha =
                return_param(|x| gl.GetIntegerv(gl::BLEND_EQUATION_ALPHA, x));
            let last_enable_blend = gl.IsEnabled(gl::BLEND) == gl::TRUE;
            let last_enable_cull_face = gl.IsEnabled(gl::CULL_FACE) == gl::TRUE;
            let last_enable_depth_test = gl.IsEnabled(gl::DEPTH_TEST) == gl::TRUE;
            let last_enable_scissor_test = gl.IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;

            gl.Enable(gl::BLEND);
            gl.BlendEquation(gl::FUNC_ADD);
            gl.BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl.Disable(gl::CULL_FACE);
            gl.Disable(gl::DEPTH_TEST);
            gl.Enable(gl::SCISSOR_TEST);
            gl.PolygonMode(gl::FRONT_AND_BACK, gl::FILL);

            let [width, height] = ctx.io().display_size;
            let [scale_w, scale_h] = ctx.io().display_framebuffer_scale;

            let fb_width = width * scale_w;
            let fb_height = height * scale_h;

            gl.Viewport(0, 0, fb_width as _, fb_height as _);
            let matrix = [
                [2.0 / width, 0.0, 0.0, 0.0],
                [0.0, 2.0 / -height, 0.0, 0.0],
                [0.0, 0.0, -1.0, 0.0],
                [-1.0, 1.0, 0.0, 1.0],
            ];
            gl.UseProgram(self.program);
            gl.Uniform1i(self.locs.texture, 0);
            gl.UniformMatrix4fv(self.locs.proj_mtx, 1, gl::FALSE, matrix.as_ptr() as _);
            if gl.BindSampler.is_loaded() {
                gl.BindSampler(0, 0);
            }

            gl.BindVertexArray(self.vao);
            gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl.EnableVertexAttribArray(self.locs.position);
            gl.EnableVertexAttribArray(self.locs.uv);
            gl.EnableVertexAttribArray(self.locs.color);
            gl.VertexAttribPointer(
                self.locs.position,
                2,
                gl::FLOAT,
                gl::FALSE,
                mem::size_of::<DrawVert>() as _,
                offset_of!(DrawVert, pos) as _,
            );
            gl.VertexAttribPointer(
                self.locs.uv,
                2,
                gl::FLOAT,
                gl::FALSE,
                mem::size_of::<DrawVert>() as _,
                offset_of!(DrawVert, uv) as _,
            );
            gl.VertexAttribPointer(
                self.locs.color,
                4,
                gl::UNSIGNED_BYTE,
                gl::TRUE,
                mem::size_of::<DrawVert>() as _,
                offset_of!(DrawVert, col) as _,
            );

            let draw_data = ctx.render();

            for draw_list in draw_data.draw_lists() {
                let vtx_buffer = draw_list.vtx_buffer();
                let idx_buffer = draw_list.idx_buffer();

                gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);
                gl.BufferData(
                    gl::ARRAY_BUFFER,
                    mem::size_of_val(vtx_buffer) as _,
                    vtx_buffer.as_ptr() as _,
                    gl::STREAM_DRAW,
                );

                gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
                gl.BufferData(
                    gl::ELEMENT_ARRAY_BUFFER,
                    mem::size_of_val(idx_buffer) as _,
                    idx_buffer.as_ptr() as _,
                    gl::STREAM_DRAW,
                );

                for cmd in draw_list.commands() {
                    match cmd {
                        DrawCmd::Elements {
                            count,
                            cmd_params:
                                DrawCmdParams {
                                    clip_rect: [x, y, z, w],
                                    texture_id,
                                    idx_offset,
                                    ..
                                },
                        } => {
                            gl.BindTexture(gl::TEXTURE_2D, texture_id.id() as _);

                            gl.Scissor(
                                (x * scale_w) as GLint,
                                (fb_height - w * scale_h) as GLint,
                                ((z - x) * scale_w) as GLint,
                                ((w - y) * scale_h) as GLint,
                            );

                            let idx_size = if mem::size_of::<DrawIdx>() == 2 {
                                gl::UNSIGNED_SHORT
                            } else {
                                gl::UNSIGNED_INT
                            };

                            gl.DrawElements(
                                gl::TRIANGLES,
                                count as _,
                                idx_size,
                                (idx_offset * mem::size_of::<DrawIdx>()) as _,
                            );
                        }
                        DrawCmd::ResetRenderState => {
                            // Re-apply the imgui render state setup
                            gl.Enable(gl::BLEND);
                            gl.BlendEquation(gl::FUNC_ADD);
                            gl.BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                            gl.Disable(gl::CULL_FACE);
                            gl.Disable(gl::DEPTH_TEST);
                            gl.Enable(gl::SCISSOR_TEST);
                            gl.PolygonMode(gl::FRONT_AND_BACK, gl::FILL);

                            gl.Viewport(0, 0, fb_width as _, fb_height as _);
                            gl.UseProgram(self.program);
                            gl.Uniform1i(self.locs.texture, 0);
                            gl.UniformMatrix4fv(
                                self.locs.proj_mtx,
                                1,
                                gl::FALSE,
                                matrix.as_ptr() as _,
                            );
                            if gl.BindSampler.is_loaded() {
                                gl.BindSampler(0, 0);
                            }

                            gl.BindVertexArray(self.vao);
                            gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);
                            gl.EnableVertexAttribArray(self.locs.position);
                            gl.EnableVertexAttribArray(self.locs.uv);
                            gl.EnableVertexAttribArray(self.locs.color);
                            gl.VertexAttribPointer(
                                self.locs.position,
                                2,
                                gl::FLOAT,
                                gl::FALSE,
                                mem::size_of::<DrawVert>() as _,
                                offset_of!(DrawVert, pos) as _,
                            );
                            gl.VertexAttribPointer(
                                self.locs.uv,
                                2,
                                gl::FLOAT,
                                gl::FALSE,
                                mem::size_of::<DrawVert>() as _,
                                offset_of!(DrawVert, uv) as _,
                            );
                            gl.VertexAttribPointer(
                                self.locs.color,
                                4,
                                gl::UNSIGNED_BYTE,
                                gl::TRUE,
                                mem::size_of::<DrawVert>() as _,
                                offset_of!(DrawVert, col) as _,
                            );
                        }
                        DrawCmd::RawCallback { callback, raw_cmd } => {
                            use imgui::internal::RawWrapper;
                            callback(draw_list.raw() as *const _ as _, raw_cmd);
                        }
                    }
                }
            }

            gl.UseProgram(last_program as _);
            gl.BindTexture(gl::TEXTURE_2D, last_texture as _);
            if gl.BindSampler.is_loaded() {
                gl.BindSampler(0, last_sampler as _);
            }
            gl.ActiveTexture(last_active_texture as _);
            gl.BindVertexArray(last_vertex_array as _);
            gl.BindBuffer(gl::ARRAY_BUFFER, last_array_buffer as _);
            gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, last_element_array_buffer as _);
            gl.BlendEquationSeparate(
                last_blend_equation_rgb as _,
                last_blend_equation_alpha as _,
            );
            gl.BlendFuncSeparate(
                last_blend_src_rgb as _,
                last_blend_dst_rgb as _,
                last_blend_src_alpha as _,
                last_blend_dst_alpha as _,
            );
            if last_enable_blend {
                gl.Enable(gl::BLEND)
            } else {
                gl.Disable(gl::BLEND)
            };
            if last_enable_cull_face {
                gl.Enable(gl::CULL_FACE)
            } else {
                gl.Disable(gl::CULL_FACE)
            };
            if last_enable_depth_test {
                gl.Enable(gl::DEPTH_TEST)
            } else {
                gl.Disable(gl::DEPTH_TEST)
            };
            if last_enable_scissor_test {
                gl.Enable(gl::SCISSOR_TEST)
            } else {
                gl.Disable(gl::SCISSOR_TEST)
            };
            gl.PolygonMode(gl::FRONT_AND_BACK, last_polygon_mode[0] as _);
            gl.Viewport(
                last_viewport[0] as _,
                last_viewport[1] as _,
                last_viewport[2] as _,
                last_viewport[3] as _,
            );
            gl.Scissor(
                last_scissor_box[0] as _,
                last_scissor_box[1] as _,
                last_scissor_box[2] as _,
                last_scissor_box[3] as _,
            );
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let gl = &self.gl;

        unsafe {
            gl.DeleteBuffers(1, &self.vbo);
            gl.DeleteBuffers(1, &self.ebo);
            gl.DeleteVertexArrays(1, &self.vao);
            gl.DeleteProgram(self.program);
            gl.DeleteTextures(1, &self.font_texture);
        }
    }
}

unsafe fn check_shader_compile(
    gl: &gl::Gl,
    shader: GLuint,
    stage: &str,
) -> Result<(), RendererError> {
    let mut success: GLint = 0;
    gl.GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
    if success != gl::TRUE as GLint {
        let mut len: GLint = 0;
        gl.GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl.GetShaderInfoLog(shader, len, ptr::null_mut(), buf.as_mut_ptr() as _);
        let msg = CStr::from_ptr(buf.as_ptr() as _)
            .to_string_lossy()
            .into_owned();
        return Err(RendererError::ShaderCompilation(format!(
            "{stage} shader: {msg}"
        )));
    }
    Ok(())
}

unsafe fn check_program_link(gl: &gl::Gl, program: GLuint) -> Result<(), RendererError> {
    let mut success: GLint = 0;
    gl.GetProgramiv(program, gl::LINK_STATUS, &mut success);
    if success != gl::TRUE as GLint {
        let mut len: GLint = 0;
        gl.GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len as usize];
        gl.GetProgramInfoLog(program, len, ptr::null_mut(), buf.as_mut_ptr() as _);
        let msg = CStr::from_ptr(buf.as_ptr() as _)
            .to_string_lossy()
            .into_owned();
        return Err(RendererError::ProgramLinking(msg));
    }
    Ok(())
}

fn return_param<T, F>(f: F) -> T
where
    F: FnOnce(&mut T),
{
    let mut val = unsafe { mem::zeroed() };
    f(&mut val);
    val
}
