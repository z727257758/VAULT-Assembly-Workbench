# P1S 0.4 mm reference profile

Derived from the user-provided `M4 Module 1F.3mf`, saved by Bambu Studio 02.05.00.66. Only `Metadata/project_settings.config` is retained: no meshes, images, plate assignments or sliced G-code were copied. Machine/filament start/end G-code remains part of the editable print settings. The machine/process reference is preserved. The six filament slots now use the official **Bambu PETG Basic** preset resolved from the locally installed Bambu Studio 02.05.00.66 / BBL profile package 02.05.00.05. The original custom filament parameters and identities are replaced; project RGB colors remain editable.

The P1S profile retains its official single-nozzle offset `0x2`. Bambu subtracts this offset from model coordinates when writing G-code. Packing intersects the model-space bed with the translated machine-space bed, and protects the front-left keep-out in both coordinate systems. With the default 0.5 mm margin, model Y must be at least 2.5 mm. This applies to rotation, overflow plates and optional centering; printer calibration is not changed to hide a packing error.

The exporter overlays the selected plate, layer height, wall loops, infill, brim and per-object filament/support settings. Each source model can use a selected filament slot, with unassigned parts following the default slot. Six editable RGB colors are written to the project. Plates are grouped by the complete material set (even for equal RGB values). Text and body regions share one object and plate; each material set may overflow to additional plates. Copies of one source share its slot. Printing is by layer; prime tower and skirts are disabled. Bed area is 256 × 256 mm, front-left keep-out 18 × 28 mm, and reference-profile printable height 250 mm. The template is embedded in the executable and does not require the original 3MF or a Bambu Studio installation at runtime.

`Application=BambuStudio-02.05.00.66` is a compatibility marker required by Bambu's project importer; `VAULT:Generator` identifies the actual exporter. Each copy uses a unique component wrapper with an explicit plate membership. Multi-plate origins use a 1.2 × bed-size stride, columns rounded up from sqrt(plate count).

Format references: https://github.com/bambulab/BambuStudio/blob/master/src/libslic3r/Format/bbs_3mf.cpp and https://github.com/bambulab/BambuStudio/blob/master/src/slic3r/GUI/PartPlate.cpp .

Lettering colors are stored per source separately from the body. The exporter partitions the finished solid into closed, non-overlapping material regions, reusing the same origin for all volumes. Raised text includes a 0.4 mm underlayer; engraved text colors 0.4 mm below the recessed floor without filling it. Bambu model part metadata carries each extruder assignment. Inactive lettering does not create a multi-material object.

## Bambu PETG Basic material source

Preset: `Bambu PETG Basic @BBL X1C`, setting ID `GFSG00_01`, filament ID `GFG00`. This official preset explicitly includes `Bambu Lab P1S 0.4 nozzle` in its compatible printers. Its inheritance chain is `fdm_filament_common` → `fdm_filament_pet` → `Bambu PETG Basic @base` → `Bambu PETG Basic @BBL X1C`, including `fdm_filament_template_direct_dual` before the leaf overrides. Source directory: Bambu Studio `resources/profiles/BBL/filament/` (https://github.com/bambulab/BambuStudio/tree/master/resources/profiles/BBL/filament).

Filament parameter arrays are expanded to all six project slots using the first (Direct Drive Standard) entry for P1S. All inherited filament parameters, including temperature, fan, flow, retraction and filament G-code fields, are resolved into the embedded project template. No Bambu Studio installation is required at runtime. The profile reference and `GFG00` identity are written for each color slot; the machine remains P1S 0.4 mm.

Reference values: initial-layer nozzle 245 °C, subsequent nozzle 255 °C, textured/high-temperature bed 70 °C, flow ratio 0.95, density 1.25 g/cm³, maximum volumetric speed 15 mm³/s. Palette colors, object/text material assignments and label-only plate separation remain project settings.

## Start purge-line clearance

Packing reserves the L-shaped nozzle-load extrusion in the embedded `machine_start_gcode`: X=18..240 at Y=1..1.5, and X=239.5..240 up to Y=11 (machine mm). The two rectangles receive 2 mm fixed clearance plus configured margin and part gap, and are protected in both model and nozzle-offset machine coordinates. The model footprint already includes the selected Brim. Initial packing, overflow plates and optional centering use the same exclusions. This does not modify the start G-code or reserve an entire front strip. If the embedded start routine changes, its coordinate regression test and exclusion rectangles must be updated together.

## Whole-plate convex hull

Bambu Studio 02.05.00.66 checks the convex hull of all non-custom extrusion paths against the front-left exclusion in both preview renderers (`src/slic3r/GUI/GCodeRenderer/{LegacyRenderer,AdvancedRenderer}.cpp`). Individually disjoint object footprints are insufficient: the convex hull connecting a left-side baseplate and a front cap can intersect the excluded corner. Packing now rejects candidate additions whose whole-plate footprint hull crosses the forbidden corner in either model or nozzle coordinates, with the configured margin. Centering and the final layout validation apply the same rule. The rectangular footprints conservatively include Brim plus the embedded `brim_object_gap`; actual slicer-generated supports still require a sliced-path check.

3MF packing dimensions and shared part origins come from the final manufacturing vertices, including all material regions, rather than cached mesh sizes. STL input remains unchanged.

## Parallel elongated parts

After safe MaxRects packing, a plate-local preference groups Runner Guide/Slide or repeated copies of another named part with XY aspect ratio >= 3. It tries a single end-aligned parallel band along X and Y, using the actual long axes, existing Brim envelopes and part gap. Other parts stay fixed. Legal candidates are ranked by whole-plate bounding-box area, then remaining short-side space. Rotation locks, exclusions and whole-plate hull clearance remain mandatory. If no band fits, the original placement remains. Membership, materials and plate count never change. Hulls are rebuilt before optional centering, and the final layout also checks pairwise spacing.
