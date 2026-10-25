# P1S 0.4 mm reference profile

Derived from the user-provided `M4 Module 1F.3mf`, saved by Bambu Studio 02.05.00.66. Only `Metadata/project_settings.config` is retained: no meshes, images, plate assignments or sliced G-code were copied. Machine/filament start/end G-code remains part of the editable print settings. Custom filament preset names were replaced with VAULT names; six PETG filament parameter/color slots are preserved.

The exporter overlays the selected plate, layer height, wall loops, infill, brim and per-object filament/support settings. Each source model can use a selected filament slot, with unassigned parts following the default slot. Six editable RGB colors are written to the project. Plates are grouped by the complete material set (even for equal RGB values). Text and body regions share one object and plate; each material set may overflow to additional plates. Copies of one source share its slot. Printing is by layer; prime tower and skirts are disabled. Bed area is 256 × 256 mm, front-left keep-out 18 × 28 mm, and reference-profile printable height 250 mm. The template is embedded in the executable and does not require the original 3MF or a Bambu Studio installation at runtime.

`Application=BambuStudio-02.05.00.66` is a compatibility marker required by Bambu's project importer; `VAULT:Generator` identifies the actual exporter. Each copy uses a unique component wrapper with an explicit plate membership. Multi-plate origins use a 1.2 × bed-size stride, columns rounded up from sqrt(plate count).

Format references: https://github.com/bambulab/BambuStudio/blob/master/src/libslic3r/Format/bbs_3mf.cpp and https://github.com/bambulab/BambuStudio/blob/master/src/slic3r/GUI/PartPlate.cpp .

Lettering colors are stored per source separately from the body. The exporter partitions the finished solid into closed, non-overlapping material regions, reusing the same origin for all volumes. Raised text includes a 0.4 mm underlayer; engraved text colors 0.4 mm below the recessed floor without filling it. Bambu model part metadata carries each extruder assignment. Inactive lettering does not create a multi-material object.
