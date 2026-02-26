# Task 21: Module System

## Goal
Implement multi-file compilation with `import` statements, `pub` visibility, and stdlib path resolution, enabling `import std::io;` and `import my_module;`.

## Requirements

### Import Syntax
- Basic: `import std::io;` — imports module, access via `io::println`
- Grouped: `import std::io::{print, println};` — import specific items
- Aliased: `import std::io::println as p;` — rename
- Relative: `import super::utils;` — parent module
- File resolution: `import foo` looks for `foo.nudl` in same directory or `foo/mod.nudl`

### Visibility
- `pub fn`, `pub struct`, `pub const`, `pub enum` — visible to importers
- Default: private (only visible within the defining module)
- `pub` on struct fields: `pub x: i32` — field visible to importers
- No `pub` = private field (accessible only within the module + impl blocks)

### Module Resolution Pipeline
1. Parse import statements
2. Locate source files
3. Parse imported files (recursively for transitive imports)
4. Merge declarations into a unified scope with module prefixes
5. Type-check the combined program
6. Lower to a single IR Program
7. Codegen as a single compilation unit

### Stdlib Resolution
- `import std::io` → look for `std/io.nudl` in stdlib directory
- Stdlib path: configurable via environment variable or default to `<install>/lib/std/`
- Prelude: automatically import common items (print, assert) without explicit import

### Scope Rules
- Each module has its own namespace
- Imported items are added to the importing module's scope
- Name collisions require aliasing
- Circular imports: detect and error

### IR Impact
- Multiple files' IR functions are merged into a single `Program`
- Function names include module prefix for disambiguation: `module__function`
- Or use a module ID system

## Acceptance Criteria
- `tests/modules/basic-import/` — import a function from another file
- `tests/modules/grouped-import/` — import multiple items
- `tests/modules/aliased-import/` — aliased imports work
- `tests/modules/visibility/` — private items are not importable
- Multi-file project compiles to single binary
- Stdlib imports resolve correctly
- Circular import detection works

## Technical Notes
- The compilation pipeline (`nudl-cli/src/pipeline.rs`) currently handles single files
- Need to extend to discover and parse multiple files
- Each file becomes a module; directory with `mod.nudl` becomes a module with submodules
- The type checker needs a module-aware scope: `ModuleScope { parent: Option<ModuleId>, items: HashMap<Symbol, Item> }`
- The lowerer merges all modules' functions into a single Program with mangled names
- For the linker: all functions end up in one compilation unit, no separate object files needed in MVP
- Depends on: most other features should work before module system ties them together
