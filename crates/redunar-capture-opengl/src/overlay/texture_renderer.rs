use super::ApiFlavor;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::sync::{LazyLock, Mutex, OnceLock};

const GL_FALSE: u8 = 0;
const GL_TRUE: u8 = 1;
const GL_FLOAT: c_uint = 0x1406;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const GL_RGBA: c_uint = 0x1908;
const GL_TEXTURE_2D: c_uint = 0x0DE1;
const GL_TEXTURE0: c_uint = 0x84C0;
const GL_TEXTURE_MIN_FILTER: c_uint = 0x2801;
const GL_TEXTURE_MAG_FILTER: c_uint = 0x2800;
const GL_LINEAR: c_int = 0x2601;
const GL_CLAMP_TO_EDGE: c_int = 0x812F;
const GL_TEXTURE_WRAP_S: c_uint = 0x2802;
const GL_TEXTURE_WRAP_T: c_uint = 0x2803;
const GL_ARRAY_BUFFER: c_uint = 0x8892;
const GL_ARRAY_BUFFER_BINDING: c_uint = 0x8894;
const GL_VERTEX_ARRAY_BINDING: c_uint = 0x85B5;
const GL_STATIC_DRAW: c_uint = 0x88E4;
const GL_VERTEX_SHADER: c_uint = 0x8B31;
const GL_FRAGMENT_SHADER: c_uint = 0x8B30;
const GL_COMPILE_STATUS: c_uint = 0x8B81;
const GL_LINK_STATUS: c_uint = 0x8B82;
const GL_CURRENT_PROGRAM: c_uint = 0x8B8D;
const GL_ACTIVE_TEXTURE: c_uint = 0x84E0;
const GL_TEXTURE_BINDING_2D: c_uint = 0x8069;
const GL_BLEND: c_uint = 0x0BE2;
const GL_CULL_FACE: c_uint = 0x0B44;
const GL_DEPTH_TEST: c_uint = 0x0B71;
const GL_STENCIL_TEST: c_uint = 0x0B90;
const GL_SCISSOR_TEST: c_uint = 0x0C11;
const GL_SCISSOR_BOX: c_uint = 0x0C10;
const GL_COLOR_WRITEMASK: c_uint = 0x0C23;
const GL_BLEND_SRC_RGB: c_uint = 0x80C9;
const GL_BLEND_DST_RGB: c_uint = 0x80C8;
const GL_BLEND_SRC_ALPHA: c_uint = 0x80CB;
const GL_BLEND_DST_ALPHA: c_uint = 0x80CA;
const GL_BLEND_EQUATION_RGB: c_uint = 0x8009;
const GL_BLEND_EQUATION_ALPHA: c_uint = 0x883D;
const GL_ONE_MINUS_SRC_ALPHA: c_uint = 0x0303;
const GL_ONE: c_uint = 1;
const GL_ZERO: c_uint = 0;
const GL_FUNC_ADD: c_uint = 0x8006;
const GL_TRIANGLE_STRIP: c_uint = 0x0005;
const GL_FRAMEBUFFER: c_uint = 0x8D40;
const GL_DRAW_FRAMEBUFFER: c_uint = 0x8CA9;
const GL_FRAMEBUFFER_BINDING: c_uint = 0x8CA6;
const GL_VERSION: c_uint = 0x1F02;
const GL_VIEWPORT: c_uint = 0x0BA2;
const MAX_CONTEXTS: usize = 8;

type GetInteger = unsafe extern "C" fn(c_uint, *mut c_int);
type GetBoolean = unsafe extern "C" fn(c_uint, *mut u8);
type GetString = unsafe extern "C" fn(c_uint) -> *const u8;
type IsEnabled = unsafe extern "C" fn(c_uint) -> u8;
type EnableDisable = unsafe extern "C" fn(c_uint);
type Scissor = unsafe extern "C" fn(c_int, c_int, c_int, c_int);
type ColorMask = unsafe extern "C" fn(u8, u8, u8, u8);
type BindFramebuffer = unsafe extern "C" fn(c_uint, c_uint);
type CreateShader = unsafe extern "C" fn(c_uint) -> c_uint;
type ShaderSource = unsafe extern "C" fn(c_uint, c_int, *const *const c_char, *const c_int);
type CompileShader = unsafe extern "C" fn(c_uint);
type GetShaderIv = unsafe extern "C" fn(c_uint, c_uint, *mut c_int);
type DeleteShader = unsafe extern "C" fn(c_uint);
type CreateProgram = unsafe extern "C" fn() -> c_uint;
type AttachShader = unsafe extern "C" fn(c_uint, c_uint);
type BindAttribLocation = unsafe extern "C" fn(c_uint, c_uint, *const c_char);
type LinkProgram = unsafe extern "C" fn(c_uint);
type GetProgramIv = unsafe extern "C" fn(c_uint, c_uint, *mut c_int);
type DeleteProgram = unsafe extern "C" fn(c_uint);
type UseProgram = unsafe extern "C" fn(c_uint);
type GetUniformLocation = unsafe extern "C" fn(c_uint, *const c_char) -> c_int;
type Uniform1i = unsafe extern "C" fn(c_int, c_int);
type Uniform4f = unsafe extern "C" fn(c_int, f32, f32, f32, f32);
type GenObjects = unsafe extern "C" fn(c_int, *mut c_uint);
type DeleteObjects = unsafe extern "C" fn(c_int, *const c_uint);
type BindBuffer = unsafe extern "C" fn(c_uint, c_uint);
type BufferData = unsafe extern "C" fn(c_uint, isize, *const c_void, c_uint);
type BindTexture = unsafe extern "C" fn(c_uint, c_uint);
type ActiveTexture = unsafe extern "C" fn(c_uint);
type TexParameteri = unsafe extern "C" fn(c_uint, c_uint, c_int);
type TexImage2d =
    unsafe extern "C" fn(c_uint, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint, *const c_void);
type EnableVertexAttribArray = unsafe extern "C" fn(c_uint);
type VertexAttribPointer = unsafe extern "C" fn(c_uint, c_int, c_uint, u8, c_int, *const c_void);
type DrawArrays = unsafe extern "C" fn(c_uint, c_int, c_int);
type BlendFuncSeparate = unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint);
type BlendEquationSeparate = unsafe extern "C" fn(c_uint, c_uint);
type BindVertexArray = unsafe extern "C" fn(c_uint);

#[derive(Clone, Copy)]
struct Functions {
    get_integer: GetInteger,
    get_boolean: GetBoolean,
    get_string: GetString,
    is_enabled: IsEnabled,
    enable: EnableDisable,
    disable: EnableDisable,
    scissor: Scissor,
    color_mask: ColorMask,
    bind_framebuffer: BindFramebuffer,
    create_shader: CreateShader,
    shader_source: ShaderSource,
    compile_shader: CompileShader,
    get_shader_iv: GetShaderIv,
    delete_shader: DeleteShader,
    create_program: CreateProgram,
    attach_shader: AttachShader,
    bind_attrib_location: BindAttribLocation,
    link_program: LinkProgram,
    get_program_iv: GetProgramIv,
    delete_program: DeleteProgram,
    use_program: UseProgram,
    get_uniform_location: GetUniformLocation,
    uniform_1i: Uniform1i,
    uniform_4f: Uniform4f,
    gen_buffers: GenObjects,
    delete_buffers: DeleteObjects,
    bind_buffer: BindBuffer,
    buffer_data: BufferData,
    gen_textures: GenObjects,
    delete_textures: DeleteObjects,
    bind_texture: BindTexture,
    active_texture: ActiveTexture,
    tex_parameter_i: TexParameteri,
    tex_image_2d: TexImage2d,
    enable_vertex_attrib_array: EnableVertexAttribArray,
    vertex_attrib_pointer: VertexAttribPointer,
    draw_arrays: DrawArrays,
    blend_func_separate: BlendFuncSeparate,
    blend_equation_separate: BlendEquationSeparate,
    gen_vertex_arrays: Option<GenObjects>,
    delete_vertex_arrays: Option<DeleteObjects>,
    bind_vertex_array: Option<BindVertexArray>,
}

impl Functions {
    fn load() -> Option<Self> {
        Some(Self {
            get_integer: super::super::resolve_gl_function(b"glGetIntegerv\0")?,
            get_boolean: super::super::resolve_gl_function(b"glGetBooleanv\0")?,
            get_string: super::super::resolve_gl_function(b"glGetString\0")?,
            is_enabled: super::super::resolve_gl_function(b"glIsEnabled\0")?,
            enable: super::super::resolve_gl_function(b"glEnable\0")?,
            disable: super::super::resolve_gl_function(b"glDisable\0")?,
            scissor: super::super::resolve_gl_function(b"glScissor\0")?,
            color_mask: super::super::resolve_gl_function(b"glColorMask\0")?,
            bind_framebuffer: super::super::resolve_gl_function(b"glBindFramebuffer\0")?,
            create_shader: super::super::resolve_gl_function(b"glCreateShader\0")?,
            shader_source: super::super::resolve_gl_function(b"glShaderSource\0")?,
            compile_shader: super::super::resolve_gl_function(b"glCompileShader\0")?,
            get_shader_iv: super::super::resolve_gl_function(b"glGetShaderiv\0")?,
            delete_shader: super::super::resolve_gl_function(b"glDeleteShader\0")?,
            create_program: super::super::resolve_gl_function(b"glCreateProgram\0")?,
            attach_shader: super::super::resolve_gl_function(b"glAttachShader\0")?,
            bind_attrib_location: super::super::resolve_gl_function(b"glBindAttribLocation\0")?,
            link_program: super::super::resolve_gl_function(b"glLinkProgram\0")?,
            get_program_iv: super::super::resolve_gl_function(b"glGetProgramiv\0")?,
            delete_program: super::super::resolve_gl_function(b"glDeleteProgram\0")?,
            use_program: super::super::resolve_gl_function(b"glUseProgram\0")?,
            get_uniform_location: super::super::resolve_gl_function(b"glGetUniformLocation\0")?,
            uniform_1i: super::super::resolve_gl_function(b"glUniform1i\0")?,
            uniform_4f: super::super::resolve_gl_function(b"glUniform4f\0")?,
            gen_buffers: super::super::resolve_gl_function(b"glGenBuffers\0")?,
            delete_buffers: super::super::resolve_gl_function(b"glDeleteBuffers\0")?,
            bind_buffer: super::super::resolve_gl_function(b"glBindBuffer\0")?,
            buffer_data: super::super::resolve_gl_function(b"glBufferData\0")?,
            gen_textures: super::super::resolve_gl_function(b"glGenTextures\0")?,
            delete_textures: super::super::resolve_gl_function(b"glDeleteTextures\0")?,
            bind_texture: super::super::resolve_gl_function(b"glBindTexture\0")?,
            active_texture: super::super::resolve_gl_function(b"glActiveTexture\0")?,
            tex_parameter_i: super::super::resolve_gl_function(b"glTexParameteri\0")?,
            tex_image_2d: super::super::resolve_gl_function(b"glTexImage2D\0")?,
            enable_vertex_attrib_array: super::super::resolve_gl_function(
                b"glEnableVertexAttribArray\0",
            )?,
            vertex_attrib_pointer: super::super::resolve_gl_function(b"glVertexAttribPointer\0")?,
            draw_arrays: super::super::resolve_gl_function(b"glDrawArrays\0")?,
            blend_func_separate: super::super::resolve_gl_function(b"glBlendFuncSeparate\0")?,
            blend_equation_separate: super::super::resolve_gl_function(
                b"glBlendEquationSeparate\0",
            )?,
            gen_vertex_arrays: super::super::resolve_gl_function(b"glGenVertexArrays\0")
                .or_else(|| super::super::resolve_gl_function(b"glGenVertexArraysOES\0")),
            delete_vertex_arrays: super::super::resolve_gl_function(b"glDeleteVertexArrays\0")
                .or_else(|| super::super::resolve_gl_function(b"glDeleteVertexArraysOES\0")),
            bind_vertex_array: super::super::resolve_gl_function(b"glBindVertexArray\0")
                .or_else(|| super::super::resolve_gl_function(b"glBindVertexArrayOES\0")),
        })
    }
}

/// One resources entry keeps one texture per overlay surface so the metrics
/// panel, the Replay menu, and the saved notice can cache their pixel
/// revisions independently within the same context.
const SURFACE_COUNT: usize = 4;
pub(super) const SURFACE_METRICS: usize = 0;
pub(super) const SURFACE_NOTICE: usize = 1;
pub(super) const SURFACE_MENU: usize = 2;
pub(super) const SURFACE_CURSOR: usize = 3;

struct Resources {
    context: usize,
    program: c_uint,
    textures: [c_uint; SURFACE_COUNT],
    buffer: c_uint,
    vertex_array: c_uint,
    rect_uniform: c_int,
    texture_uniform: c_int,
    uploaded_revisions: [u64; SURFACE_COUNT],
}

static FUNCTIONS: OnceLock<Option<Functions>> = OnceLock::new();
static RESOURCES: LazyLock<Mutex<Vec<Resources>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub(super) fn destroy_context(context: usize) {
    if context == 0 {
        return;
    }
    let mut entries = RESOURCES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.retain(|entry| entry.context != context);
}

pub(super) fn viewport() -> Option<[i32; 4]> {
    let functions = (*FUNCTIONS.get_or_init(Functions::load))?;
    let mut viewport = [0; 4];
    unsafe { (functions.get_integer)(GL_VIEWPORT, viewport.as_mut_ptr()) };
    Some(viewport)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the GL target and prepared surface are explicit"
)]
pub(super) unsafe fn draw(
    context: usize,
    api: ApiFlavor,
    surface: usize,
    viewport: [i32; 4],
    origin: (i32, i32),
    width: i32,
    height: i32,
    revision: u64,
    pixels: &[u8],
) -> bool {
    if context == 0 || width <= 0 || height <= 0 {
        return false;
    }
    let Some(functions) = *FUNCTIONS.get_or_init(Functions::load) else {
        return false;
    };
    let Ok(mut entries) = RESOURCES.try_lock() else {
        return false;
    };
    let index = if let Some(index) = entries.iter().position(|entry| entry.context == context) {
        index
    } else {
        if entries.len() >= MAX_CONTEXTS {
            return false;
        }
        let Some(resources) = (unsafe { create_resources(&functions, context, api) }) else {
            return false;
        };
        entries.push(resources);
        entries.len() - 1
    };
    let resources = &mut entries[index];
    let surface = surface.min(SURFACE_COUNT - 1);
    unsafe {
        draw_with(
            &functions,
            resources,
            surface,
            viewport,
            origin,
            width,
            height,
            revision,
            pixels,
            (origin.0, origin.1, width, height),
        )
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the GL target, prepared surface, and containing clip are explicit"
)]
pub(super) unsafe fn draw_clipped(
    context: usize,
    api: ApiFlavor,
    surface: usize,
    viewport: [i32; 4],
    origin: (i32, i32),
    width: i32,
    height: i32,
    revision: u64,
    pixels: &[u8],
    clip: (i32, i32, i32, i32),
) -> bool {
    if context == 0 || width <= 0 || height <= 0 || clip.2 <= 0 || clip.3 <= 0 {
        return false;
    }
    let Some(functions) = *FUNCTIONS.get_or_init(Functions::load) else {
        return false;
    };
    let Ok(mut entries) = RESOURCES.try_lock() else {
        return false;
    };
    let index = if let Some(index) = entries.iter().position(|entry| entry.context == context) {
        index
    } else {
        if entries.len() >= MAX_CONTEXTS {
            return false;
        }
        let Some(resources) = (unsafe { create_resources(&functions, context, api) }) else {
            return false;
        };
        entries.push(resources);
        entries.len() - 1
    };
    let resources = &mut entries[index];
    let surface = surface.min(SURFACE_COUNT - 1);
    unsafe {
        draw_with(
            &functions, resources, surface, viewport, origin, width, height, revision, pixels, clip,
        )
    }
}

unsafe fn create_resources(
    functions: &Functions,
    context: usize,
    api: ApiFlavor,
) -> Option<Resources> {
    // Creation and cleanup must be available as a pair. Without deletion a
    // failed partial setup could accumulate VAOs in a live shared GL group.
    functions.delete_vertex_arrays?;
    let version = unsafe { version_bytes(functions) };
    let core = api == ApiFlavor::Desktop && version.windows(4).any(|part| part == b"Core");
    let (vertex_source, fragment_source) = shader_sources(api, core);
    let vertex = unsafe { compile_shader(functions, GL_VERTEX_SHADER, vertex_source)? };
    let fragment = unsafe { compile_shader(functions, GL_FRAGMENT_SHADER, fragment_source)? };
    let program = unsafe { (functions.create_program)() };
    if program == 0 {
        unsafe {
            (functions.delete_shader)(vertex);
            (functions.delete_shader)(fragment);
        }
        return None;
    }
    unsafe {
        (functions.attach_shader)(program, vertex);
        (functions.attach_shader)(program, fragment);
        (functions.bind_attrib_location)(program, 0, c"a_position".as_ptr());
        (functions.bind_attrib_location)(program, 1, c"a_uv".as_ptr());
        (functions.link_program)(program);
        (functions.delete_shader)(vertex);
        (functions.delete_shader)(fragment);
    }
    let mut linked = 0;
    unsafe { (functions.get_program_iv)(program, GL_LINK_STATUS, &raw mut linked) };
    if linked == 0 {
        unsafe { (functions.delete_program)(program) };
        return None;
    }
    let mut textures = [0_u32; SURFACE_COUNT];
    let mut buffer = 0;
    let mut vertex_array = 0;
    unsafe {
        (functions.gen_textures)(
            i32::try_from(SURFACE_COUNT).unwrap_or(1),
            textures.as_mut_ptr(),
        );
        (functions.gen_buffers)(1, &raw mut buffer);
        if let Some(generate) = functions.gen_vertex_arrays {
            generate(1, &raw mut vertex_array);
        }
    }
    let textures_ready = textures.iter().all(|name| *name != 0);
    if !textures_ready
        || buffer == 0
        || vertex_array == 0
        || functions.bind_vertex_array.is_none()
        || functions.delete_vertex_arrays.is_none()
    {
        unsafe {
            for name in textures {
                if name != 0 {
                    (functions.delete_textures)(1, &raw const name);
                }
            }
            if buffer != 0 {
                (functions.delete_buffers)(1, &raw const buffer);
            }
            if vertex_array != 0
                && let Some(delete) = functions.delete_vertex_arrays
            {
                delete(1, &raw const vertex_array);
            }
            (functions.delete_program)(program);
        }
        return None;
    }
    Some(Resources {
        context,
        program,
        textures,
        buffer,
        vertex_array,
        rect_uniform: unsafe { (functions.get_uniform_location)(program, c"u_rect".as_ptr()) },
        texture_uniform: unsafe {
            (functions.get_uniform_location)(program, c"u_texture".as_ptr())
        },
        uploaded_revisions: [0; SURFACE_COUNT],
    })
}

unsafe fn compile_shader(
    functions: &Functions,
    kind: c_uint,
    source: &'static CStr,
) -> Option<c_uint> {
    let shader = unsafe { (functions.create_shader)(kind) };
    if shader == 0 {
        return None;
    }
    let source_ptr = source.as_ptr();
    unsafe {
        (functions.shader_source)(shader, 1, &raw const source_ptr, std::ptr::null());
        (functions.compile_shader)(shader);
    }
    let mut compiled = 0;
    unsafe { (functions.get_shader_iv)(shader, GL_COMPILE_STATUS, &raw mut compiled) };
    if compiled == 0 {
        unsafe { (functions.delete_shader)(shader) };
        None
    } else {
        Some(shader)
    }
}

#[expect(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    reason = "the bounded GL state transaction and pixel-to-clip conversion stay together"
)]
unsafe fn draw_with(
    functions: &Functions,
    resources: &mut Resources,
    surface: usize,
    viewport: [i32; 4],
    origin: (i32, i32),
    width: i32,
    height: i32,
    revision: u64,
    pixels: &[u8],
    clip: (i32, i32, i32, i32),
) -> bool {
    let Some(bind_vertex_array) = functions.bind_vertex_array else {
        return false;
    };
    let mut old_program = 0;
    let mut old_buffer = 0;
    let mut old_vertex_array = 0;
    let mut old_active_texture = 0;
    let mut old_texture = 0;
    let mut old_scissor = [0; 4];
    let mut old_mask = [0_u8; 4];
    let mut old_framebuffer = 0;
    let mut old_blend = [0; 6];
    unsafe {
        (functions.get_integer)(GL_CURRENT_PROGRAM, &raw mut old_program);
        (functions.get_integer)(GL_ARRAY_BUFFER_BINDING, &raw mut old_buffer);
        (functions.get_integer)(GL_VERTEX_ARRAY_BINDING, &raw mut old_vertex_array);
        (functions.get_integer)(GL_ACTIVE_TEXTURE, &raw mut old_active_texture);
        (functions.active_texture)(GL_TEXTURE0);
        (functions.get_integer)(GL_TEXTURE_BINDING_2D, &raw mut old_texture);
        (functions.get_integer)(GL_SCISSOR_BOX, old_scissor.as_mut_ptr());
        (functions.get_boolean)(GL_COLOR_WRITEMASK, old_mask.as_mut_ptr());
        (functions.get_integer)(GL_FRAMEBUFFER_BINDING, &raw mut old_framebuffer);
        for (slot, name) in old_blend.iter_mut().zip([
            GL_BLEND_SRC_RGB,
            GL_BLEND_DST_RGB,
            GL_BLEND_SRC_ALPHA,
            GL_BLEND_DST_ALPHA,
            GL_BLEND_EQUATION_RGB,
            GL_BLEND_EQUATION_ALPHA,
        ]) {
            (functions.get_integer)(name, slot);
        }
    }
    let blend_enabled = unsafe { (functions.is_enabled)(GL_BLEND) != 0 };
    let scissor_enabled = unsafe { (functions.is_enabled)(GL_SCISSOR_TEST) != 0 };
    let cull_enabled = unsafe { (functions.is_enabled)(GL_CULL_FACE) != 0 };
    let depth_enabled = unsafe { (functions.is_enabled)(GL_DEPTH_TEST) != 0 };
    let stencil_enabled = unsafe { (functions.is_enabled)(GL_STENCIL_TEST) != 0 };
    let target = unsafe { framebuffer_target(functions) };
    let vertices: [f32; 16] = [
        0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0,
    ];
    let x = ((origin.0 - viewport[0]) as f32 / viewport[2] as f32) * 2.0 - 1.0;
    let y = ((origin.1 - viewport[1]) as f32 / viewport[3] as f32) * 2.0 - 1.0;
    let w = width as f32 * 2.0 / viewport[2] as f32;
    let h = height as f32 * 2.0 / viewport[3] as f32;
    unsafe {
        (functions.bind_framebuffer)(target, 0);
        (functions.color_mask)(GL_TRUE, GL_TRUE, GL_TRUE, GL_FALSE);
        (functions.enable)(GL_BLEND);
        (functions.disable)(GL_CULL_FACE);
        (functions.disable)(GL_DEPTH_TEST);
        (functions.disable)(GL_STENCIL_TEST);
        (functions.blend_func_separate)(GL_ONE, GL_ONE_MINUS_SRC_ALPHA, GL_ONE, GL_ZERO);
        (functions.blend_equation_separate)(GL_FUNC_ADD, GL_FUNC_ADD);
        (functions.enable)(GL_SCISSOR_TEST);
        (functions.scissor)(clip.0, clip.1, clip.2, clip.3);
        (functions.use_program)(resources.program);
        let texture = resources.textures[surface];
        (functions.active_texture)(GL_TEXTURE0);
        (functions.bind_texture)(GL_TEXTURE_2D, texture);
        (functions.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        (functions.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
        (functions.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        (functions.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        if resources.uploaded_revisions[surface] != revision {
            (functions.tex_image_2d)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA.cast_signed(),
                width,
                height,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
            resources.uploaded_revisions[surface] = revision;
        }
        (functions.uniform_1i)(resources.texture_uniform, 0);
        (functions.uniform_4f)(resources.rect_uniform, x, y, w, h);
        bind_vertex_array(resources.vertex_array);
        (functions.bind_buffer)(GL_ARRAY_BUFFER, resources.buffer);
        (functions.buffer_data)(
            GL_ARRAY_BUFFER,
            isize::try_from(std::mem::size_of_val(&vertices)).unwrap_or(0),
            vertices.as_ptr().cast(),
            GL_STATIC_DRAW,
        );
        (functions.enable_vertex_attrib_array)(0);
        (functions.enable_vertex_attrib_array)(1);
        (functions.vertex_attrib_pointer)(0, 2, GL_FLOAT, GL_FALSE, 16, std::ptr::null());
        (functions.vertex_attrib_pointer)(1, 2, GL_FLOAT, GL_FALSE, 16, 8_usize as *const c_void);
        (functions.draw_arrays)(GL_TRIANGLE_STRIP, 0, 4);
        bind_vertex_array(c_uint::from_ne_bytes(old_vertex_array.to_ne_bytes()));
        (functions.use_program)(c_uint::from_ne_bytes(old_program.to_ne_bytes()));
        (functions.bind_buffer)(
            GL_ARRAY_BUFFER,
            c_uint::from_ne_bytes(old_buffer.to_ne_bytes()),
        );
        (functions.bind_texture)(
            GL_TEXTURE_2D,
            c_uint::from_ne_bytes(old_texture.to_ne_bytes()),
        );
        (functions.active_texture)(c_uint::from_ne_bytes(old_active_texture.to_ne_bytes()));
        (functions.blend_func_separate)(
            old_blend[0].cast_unsigned(),
            old_blend[1].cast_unsigned(),
            old_blend[2].cast_unsigned(),
            old_blend[3].cast_unsigned(),
        );
        (functions.blend_equation_separate)(
            old_blend[4].cast_unsigned(),
            old_blend[5].cast_unsigned(),
        );
        if !blend_enabled {
            (functions.disable)(GL_BLEND);
        }
        if cull_enabled {
            (functions.enable)(GL_CULL_FACE);
        }
        if depth_enabled {
            (functions.enable)(GL_DEPTH_TEST);
        }
        if stencil_enabled {
            (functions.enable)(GL_STENCIL_TEST);
        }
        (functions.scissor)(
            old_scissor[0],
            old_scissor[1],
            old_scissor[2],
            old_scissor[3],
        );
        if !scissor_enabled {
            (functions.disable)(GL_SCISSOR_TEST);
        }
        (functions.color_mask)(old_mask[0], old_mask[1], old_mask[2], old_mask[3]);
        (functions.bind_framebuffer)(target, c_uint::from_ne_bytes(old_framebuffer.to_ne_bytes()));
    }
    true
}

unsafe fn version_bytes(functions: &Functions) -> &'static [u8] {
    let pointer = unsafe { (functions.get_string)(GL_VERSION) };
    if pointer.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(pointer.cast()) }.to_bytes()
    }
}

unsafe fn framebuffer_target(functions: &Functions) -> c_uint {
    let version = unsafe { version_bytes(functions) };
    if version.starts_with(b"OpenGL ES 2.")
        || version
            .first()
            .is_some_and(|major| major.is_ascii_digit() && *major < b'3')
    {
        GL_FRAMEBUFFER
    } else {
        GL_DRAW_FRAMEBUFFER
    }
}

fn shader_sources(api: ApiFlavor, core: bool) -> (&'static CStr, &'static CStr) {
    if api == ApiFlavor::Embedded {
        (c"attribute vec2 a_position; attribute vec2 a_uv; uniform vec4 u_rect; varying vec2 v_uv; void main(){v_uv=a_uv; gl_Position=vec4(u_rect.xy+a_position*u_rect.zw,0.0,1.0);}",
         c"precision mediump float; varying vec2 v_uv; uniform sampler2D u_texture; void main(){gl_FragColor=texture2D(u_texture,v_uv);}")
    } else if core {
        (c"#version 330 core\nin vec2 a_position; in vec2 a_uv; uniform vec4 u_rect; out vec2 v_uv; void main(){v_uv=a_uv; gl_Position=vec4(u_rect.xy+a_position*u_rect.zw,0.0,1.0);}",
         c"#version 330 core\nin vec2 v_uv; uniform sampler2D u_texture; out vec4 color; void main(){color=texture(u_texture,v_uv);}")
    } else {
        (c"#version 120\nattribute vec2 a_position; attribute vec2 a_uv; uniform vec4 u_rect; varying vec2 v_uv; void main(){v_uv=a_uv; gl_Position=vec4(u_rect.xy+a_position*u_rect.zw,0.0,1.0);}",
         c"#version 120\nvarying vec2 v_uv; uniform sampler2D u_texture; void main(){gl_FragColor=texture2D(u_texture,v_uv);}")
    }
}
