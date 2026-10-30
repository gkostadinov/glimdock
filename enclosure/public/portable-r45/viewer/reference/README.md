# Illustrative envelopes

These five meshes are simple authored slabs with rounded corners. They show the
nominal glass, active display, measured battery and two measured speakers in the
assembly. They are not manufacturer CAD, electrical models, printable parts or
fit-validation tools. The dashboard is synthetic artwork created in `app.js`.

Dimensions are millimetres. The illustration frame is X right, Y up, Z towards
the rear. `generate_envelopes.py` regenerates the shapes with CadQuery; the
static viewer needs no Python dependency.

| Envelope | Size | Centre |
| --- | --- | --- |
| Glass, R2 corners | 73.06 × 50.54 × 0.7 | 3, 0, 0.5 |
| Active display | 57.6 × 43.2 × 0.01 | 0, 0, 0.14 |
| Battery | 60.5 × 50.2 × 7.4 | 3, 0, 18.2 |
| Left speaker | 20 × 30 × 4.7 | −18, 8, 26.9 |
| Right speaker | 20 × 30 × 4.7 | 24, 8, 26.9 |

The enclosure and these geometry meshes use CERN-OHL-P-2.0. The generic
generator uses MIT. Keep the adjacent license notices with redistributed files.
