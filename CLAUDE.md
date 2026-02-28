# Logos (Rust)

Math-native code editor. Zig-to-Rust rewrite.

## Build & Run

```bash
cargo run        # build and run
cargo build      # build only
```

## Architecture

Single crate with module-based organization:

- `app` — Application state, winit event loop
- `render` — wgpu GPU context, text rendering via glyphon/cosmic-text
- `editor` — Text buffer, cursor, editing operations
- `ui` — Layout (Taffy), theme/colors
- `lang` — AST, lexer, parser, type checker, GLSL codegen (future)

## Tech Stack

- **winit** — Windowing and input
- **wgpu** — GPU rendering
- **cosmic-text** (via glyphon) — Text shaping and layout
- **taffy** — CSS Flexbox/Grid layout engine (future)
- **ttf-parser** — OpenType MATH table reading (future)

## Fonts

- **STIX Two Math** — Math font (OFL)
- **JuliaMono** — Code font (OFL)

## Design Decisions

- Source text is always valid code with Unicode symbols (π, ∑, etc.)
- Two display modes: Math view (default) and Code view (toggle)
- Lexer + parser run on every keystroke; type checker + codegen run on Play
- Hybrid retained/immediate rendering architecture
