# Workspace instructions

This is a standalone native Rust workspace. The VAULT STL library is external, read-only input.
Never write generated files, application code, caches, exports or test artifacts into the model library.
Keep build output in target/, runtime settings in state/, and validation artifacts in output/.
Do not copy the STL package into this repository.

Architecture: catalog.rs owns file matching; mesh.rs owns STL parsing; display.rs owns read-only display processing; scene.rs owns assembly instances;
render.rs owns wgpu resources; app.rs owns egui interaction. Keep filesystem parsing off the UI thread.
Persist calibrated assembly data separately from original mesh files. Never label estimated transforms as verified.
Use Cargo.lock and avoid dependency upgrades unrelated to the task.
Run cargo fmt, cargo test, and cargo clippy for code changes. Use the native --smoke-test for rendering changes.

constraints.rs owns the directed assembly graph and mating rules; collision.rs owns background mesh-surface crossing diagnostics. Do not describe bounding-box overlap as proven penetration or a clean surface check as a validated CAD assembly.
