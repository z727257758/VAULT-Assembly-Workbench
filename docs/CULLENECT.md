# Cullenect interface

Reference: https://github.com/CullenJWebb/Cullenect-Labels (README dimensional interface, accessed 2026-10-26).

This implementation generates solids in Rust using the existing manifold-rust dependency. It does not import or execute Cullenect SCAD sources and does not add Gridfinity Extended.

- Label: 36 × 11 × 1.2 mm, fixed 1U width, centered on the existing Rebuilt shelf.
- Perimeter latch: 0.2 mm inset, Z = 0.2–0.8 mm from the label underside.
- Socket: 36.3 × 11.3 × 1.2 mm, total XY clearance 0.3 mm.
- Two ribs along the long edges: 0.2 mm projection, 0.4 mm high, Z = 0.2–0.6 mm above the pocket floor.
- Local adaptations: rectangular profile, reinforcement under the original shelf leaving 1 mm under the pocket, and a front finger opening. The label itself is the same size for all bin widths.
- Text: 34 × 10 mm safe area; 0.2 mm recommended, limited to 0.4 mm to preserve the latch when engraved.

The old integral shelf remains available; existing saved presets do not opt in automatically. STL exports separate files and 3MF exports separate printable objects with independent material assignments. Print the single-color body and multicolor labels in separate print jobs to keep the label prime tower short.

Nominal closed-solid, clearance, assembly-placement and export checks are software validation only. Physical snap force, printer tolerances, repeated removal, and cross-compatibility with other Cullenect implementations remain unverified.

Upstream license notice retained for the referenced design:

MIT License

Copyright (c) 2024 Cullen J Webb

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
