# Bambu Lab 0.4 mm printer profiles

VAULT embeds four machine profiles. Each 3MF carries the selected machine's start, end and filament-change commands, standard process settings and material parameters. Older configurations select P1S by default.

| Machine | Printable X × Y × Z (mm) | Nozzle XY offset (mm) | Front-left exclusion (mm) | Standard process |
| --- | --- | --- | --- | --- |
| P1S | 256 × 256 × 250 | 0, 2 | 18 × 28 | 0.20mm Standard @BBL X1C |
| X1 Carbon | 256 × 256 × 250 | 0, 2 | 18 × 28 | 0.20mm Standard @BBL X1C |
| A1 | 256 × 256 × 256 | 0, 0 | — | 0.20mm Standard @BBL A1 |
| A1 mini | 180 × 180 × 180 | 0, 0 | — | 0.20mm Standard @BBL A1M |

The A1, A1 mini and X1 Carbon templates come from Bambu Studio 02.05.00.66. For each machine, the factory machine, standard process and filament JSON presets were resolved through their `inherits` and `include` chains and loaded with Studio's `--load-settings` and `--load-filaments`. Studio's `--export-settings` produced each independent project template. The base uses six PETG Basic slots. Each matching `*-filaments.json` records Studio's per-slot differences for PLA Basic, PLA Matte and PETG HF on that machine. The standard-flow nozzle selects the Direct Drive Standard column. Slot mappings and flush volumes are expanded to the actual number of configured filaments. [Bambu Studio factory profiles](https://github.com/bambulab/BambuStudio/tree/v02.05.00.66/resources/profiles/BBL)

## P1S reference project

The machine and process settings come from `Metadata/project_settings.config` in the user-provided `M4 Module 1F.3mf`, saved by Bambu Studio 02.05.00.66. The embedded project settings include editable machine and filament start/end G-code. Each filament slot has a material preset and an independent RGB color.

The P1S profile retains its official single-nozzle offset `0x2`. Bambu subtracts this offset from model coordinates when writing G-code. Packing intersects the model-space bed with the translated machine-space bed, and protects the front-left keep-out in both coordinate systems. With the default 0.5 mm margin, model Y must be at least 2.5 mm. This applies to rotation, overflow plates and optional centering; printer calibration is not changed to hide a packing error.

The exporter overlays the selected plate, layer height, wall loops, infill, brim and per-object filament/support settings. Each source model can use a selected filament slot, with unassigned parts following the default slot. The color and material of every slot are written to the project. Plates are grouped by the complete material set (even for equal RGB values). Text and body regions share one object and plate. Each material set may overflow to additional plates. Copies of one source share its slot. Printing is by layer. Prime tower and skirts are disabled. Bed area is 256 × 256 mm, front-left keep-out 18 × 28 mm, and reference-profile printable height 250 mm. The template is embedded in the executable.

`Application=BambuStudio-02.05.00.66` is a compatibility marker required by Bambu's project importer; `VAULT:Generator` identifies the actual exporter. Each copy uses a unique component wrapper with an explicit plate membership. Multi-plate origins use a 1.2 × bed-size stride, columns rounded up from sqrt(plate count).

Format references: https://github.com/bambulab/BambuStudio/blob/master/src/libslic3r/Format/bbs_3mf.cpp and https://github.com/bambulab/BambuStudio/blob/master/src/slic3r/GUI/PartPlate.cpp .

Lettering colors are stored per source separately from the body. The exporter partitions the finished solid into closed, non-overlapping material regions, reusing the same origin for all volumes. Raised text includes a 0.4 mm underlayer; engraved text colors 0.4 mm below the recessed floor without filling it. Bambu model part metadata carries each extruder assignment. Inactive lettering does not create a multi-material object.

## Bambu filament presets

The supported presets come from the installed Bambu Studio 02.05.00.66 BBL profile package **02.05.00.05**. The PLA Basic, PLA Matte and PETG HF presets target the P1S 0.4 mm nozzle. The PETG Basic X1C preset lists the P1S 0.4 mm nozzle among its compatible printers. The PETG Matte swatch collection uses the PETG Basic slicing preset, as shown in material settings. [Official BBL filament profiles](https://github.com/bambulab/BambuStudio/tree/master/resources/profiles/BBL/filament)

| Material | Exported profile name | Setting ID | Filament ID | Nozzle, initial / later (°C) | Textured bed (°C) | Flow ratio | Max flow (mm³/s) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PLA Basic | `Bambu PLA Basic @BBL P1S 0.4 nozzle` | `GFSA00_34` | `GFA00` | 220 / 220 | 55 | 0.98 | 21 |
| PLA Matte | `Bambu PLA Matte @BBL P1S 0.4 nozzle` | `GFSA01_34` | `GFA01` | 220 / 220 | 55 | 0.98 | 22 |
| PETG HF | `Bambu PETG HF @BBL P1S 0.4 nozzle` | `GFSG02_36` | `GFG02` | 230 / 245 | 70 | 0.95 | 21 |
| PETG Basic | `Bambu PETG Basic @BBL X1C` | `GFSG00_01` | `GFG00` | 245 / 255 | 70 | 0.95 | 15 |

The bundled template contains the resolved PETG Basic values. `filament-overrides.json` stores the differences for PLA Basic, PLA Matte and PETG HF after resolving preset inheritance, included templates and the final preset's overrides. Multi-drive settings use the first entry (Direct Drive Standard) for the P1S single nozzle. The exporter writes each slot's profile name, filament ID and process values, including cooling, retraction and filament G-code, into the 3MF project. Both settings files are embedded in the application.

New projects assign PETG HF to slot 2 and PLA Basic to slots 1 and 3–6. `default_filament_profile` follows slot 1. Additional group colors extend the filament columns, indexing and unload/load pairs. For new pairs of distinct slots, the flush matrix uses the largest transition volume from the reference template. Existing pairs keep their reference values. The material set used for packing consists of the slot numbers assigned to each printed object, including its lettering.

Projects support up to 32 distinct filament slots, matching the `ExtruderMax` limit in the target version of Bambu Studio. [Bambu Studio 02.05.00.66 extruder indices](https://github.com/bambulab/BambuStudio/blob/v02.05.00.66/src/libslic3r/Model.hpp#L655-L695)

## Start purge-line clearance

Packing reserves the L-shaped nozzle-load extrusion in the embedded `machine_start_gcode`: X=18..240 at Y=1..1.5, and X=239.5..240 up to Y=11 (machine mm). The two rectangles receive 2 mm fixed clearance plus configured margin and part gap, and are protected in both model and nozzle-offset machine coordinates. The model footprint already includes the selected Brim. Initial packing, overflow plates and optional centering use the same exclusions. This does not modify the start G-code or reserve an entire front strip. If the embedded start routine changes, its coordinate regression test and exclusion rectangles must be updated together.

## Whole-plate convex hull

Bambu Studio 02.05.00.66 checks the convex hull of all non-custom extrusion paths against the front-left exclusion in both preview renderers (`src/slic3r/GUI/GCodeRenderer/{LegacyRenderer,AdvancedRenderer}.cpp`). Individually disjoint object footprints are insufficient: the convex hull connecting a left-side baseplate and a front cap can intersect the excluded corner. Packing now rejects candidate additions whose whole-plate footprint hull crosses the forbidden corner in either model or nozzle coordinates, with the configured margin. Centering and the final layout validation apply the same rule. The rectangular footprints conservatively include Brim plus the embedded `brim_object_gap`; actual slicer-generated supports still require a sliced-path check.

3MF packing dimensions and shared part origins come from the final manufacturing vertices, including all material regions, rather than cached mesh sizes. STL input remains unchanged.

## Parallel elongated parts

After safe MaxRects packing, a plate-local preference groups Runner Guide/Slide or repeated copies of another named part with XY aspect ratio >= 3. It tries a single end-aligned parallel band along X and Y, using the actual long axes, existing Brim envelopes and part gap. Other parts stay fixed. Legal candidates are ranked by whole-plate bounding-box area, then remaining short-side space. Rotation locks, exclusions and whole-plate hull clearance remain mandatory. If no band fits, the original placement remains. Membership, materials and plate count never change. Hulls are rebuilt before optional centering, and the final layout also checks pairwise spacing.

## Flush text colors

`label::Style::Flat` partitions text from the top of the original solid down by the configured color depth. The body and lettering are disjoint volumes whose union retains the original shape; neither relief nor an engraved void is added. 3MF uses the existing body/text filament slots and keeps both volumes in one object. STL preserves the flat geometry only. Native preview retains coplanar glyph boundaries via Manifold vertex properties, including glyph holes; the Gridfinity STL cache restores text shading from the actual font outlines. Existing Raised/Engraved settings and defaults remain compatible.

## Face-down flat labels

`flat_text_face_down` defaults to true. Active Flat text on standalone Label.stl or detachable Cullenect labels enables a proper 180-degree X rotation on the whole build instance, followed by the optional packing Z rotation. The shared translation uses the actual final mesh height so the original text surface is at Z=0. Body and text resources and their filament assignments stay unchanged. XY footprint dimensions, keep-outs and plate membership use the same packing rules. Integrated bin shelves, Raised/Engraved text and blank labels remain upright. This print-only 3MF option persists with plate settings; it does not affect assembly/display processing or STL geometry.
