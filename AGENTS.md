# Workspace instructions

## Workspace and data

- This is a standalone Windows desktop app built with Rust, eframe/egui, and wgpu. Use Rust 1.88 or newer. Keep `Cargo.lock` and avoid unrelated dependency upgrades.
- Treat the external VAULT STL library as read-only input. Keep it outside the workspace. Never write generated files, application code, caches, exports, or test artifacts into it, and never copy the model package into this repository.
- Keep Cargo output under `target/` (configured as `target/portable-paths/`). Keep runtime settings, local projects, generated geometry caches, and calibration files under `state/`. Keep validation artifacts under `output/`. Use the user-selected destination for interactive exports.
- Keep machine-specific paths out of tracked defaults. Keep external models and generated print projects out of Git.

## Module ownership

| Module | Responsibility |
| --- | --- |
| `src/catalog.rs`, `src/catalog/` | Library matching, part classification, and bill-of-materials data |
| `src/mesh.rs`, `src/display.rs` | STL parsing and budgets in `mesh`, read-only display processing in `display` |
| `src/scene.rs`, `src/scene/` | Mesh sharing, assembly instances, and scene construction |
| `src/constraints.rs`, `src/constraints/` | Directed assembly graph, mating rules, and motion constraints |
| `src/collision.rs` | Background mesh-surface crossing diagnostics |
| `src/render.rs` | wgpu resources and rendering |
| `src/app.rs`, `src/app/` | egui interactions, project state, and coordination of preview and export jobs |
| `src/print_plate.rs`, `src/print_plate/`, `src/export.rs` | Print settings, materials and placement in `print_plate`, manufacturing geometry preparation and STL/3MF writing in `export` |

- Keep filesystem parsing and expensive geometry processing off the UI thread. Reuse the shared manufacturing mesh and print-planning paths for print-board preview and 3MF export. Keep display transforms out of exported geometry.
- Preserve compatibility with older TOML projects. Persist assembly calibration separately from original meshes. Label estimated transforms as estimates and reserve confirmed status for data tied to the saved geometry fingerprint.
- Treat bounding-box overlap as a broad-phase candidate and surface crossings as diagnostics. A clean surface check does not validate CAD assembly, continuous motion, or manufacturability. See [assembly rules](docs/ASSEMBLY_RULES.md) and [combination rules](docs/COMBINATION_RULES.md).

## Validation

- For code changes, run `cargo fmt`, `cargo test --locked`, and `cargo clippy --locked --all-targets -- -D warnings`.
- For rendering changes, also run `cargo run --release --locked -- --smoke-test` with the relevant smoke flag. Use a native window and GPU. See the [README](README.md#开发与调试) for focused smoke tests and output paths.
- Tests marked ignored require external inputs. Use `tools/test-integration.ps1` with the read-only model library, OpenSCAD, and a packing input fixture for the full external suite. Report separately which checks ran and which lacked prerequisites.
