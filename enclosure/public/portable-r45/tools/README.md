# Re-exporting the editable CAD

The included STEP and BREP files are the direct-edit CAD sources. The original
parametric construction history and private fitting references are not required
to edit these analytic shapes in a CAD application.

To generate a fresh mesh in a separate directory:

```sh
python -m pip install -r tools/requirements.txt
python tools/export_meshes.py --part usb-tunnel --output regenerated
```

Omit `--part` to export every part. Add `--native` to keep the documented CAD
frame instead of applying the recorded print orientation. This tool reads local
CAD files only; it does not connect to or control a printer.

Re-tessellation produces new facets and is not a byte-for-byte rebuild of the
canonical STLs. In particular, the current front print mesh preserves previously
checked facets. Use the supplied `stl/` files for the released geometry snapshot,
and re-check fitting surfaces, support clearance and sliced layers after edits
or a mesh rebuild. The exporter never overwrites `stl/`, `cad/` or `assembly/`.

The exporter and this documentation use MIT. The generated enclosure geometry
remains covered by CERN-OHL-P-2.0; re-exporting does not change its license.
