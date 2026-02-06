# imgui-opengl-renderer-rs

OpenGL 3.3+ renderer for [Dear ImGui](https://github.com/ocornut/imgui) via the
[imgui](https://crates.io/crates/imgui) Rust bindings.

Pair with [imgui-glfw-rs](https://crates.io/crates/imgui-glfw-rs) for a
complete GLFW + OpenGL + Dear ImGui integration.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
imgui-opengl-renderer-rs = "0.12"
```

Then in your code:

```rust
use imgui::Context;
use imgui_opengl_renderer_rs::Renderer;

let mut imgui = Context::create();

// Create the renderer using your window's GL loader
let renderer = Renderer::new(&mut imgui, |s| window.get_proc_address(s) as _)
    .expect("Failed to initialize renderer");

// In your main loop, after building the imgui frame:
renderer.render(&mut imgui);
```

## Requirements

- OpenGL 3.3 Core Profile or later
- A valid current GL context when calling `Renderer::new` and `Renderer::render`

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE_APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE_MIT) or <http://opensource.org/licenses/MIT>)

at your option.
