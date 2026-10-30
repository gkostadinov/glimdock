# Portable r45 website render sources

These illustrations use the original meshes in the public `enclosure/public/portable-r45` bundle at commit `115aa457c2c804d5e48c1b866748d57f49429dc8`. The renderer changes presentation, lighting and camera placement only; it does not reshape geometry. Glass, display, battery and speaker shapes are the package's simple authored envelopes, not manufacturer PCB CAD.

Run from a source checkout with Blender 5.0.1 or a compatible newer release. Pass the actual executable path for your platform in place of `blender`:

```sh
blender -b --python scripts/render-product-r45.py -- \
  --source /path/to/portable-r45 \
  --out rendered-assets \
  --screen scripts/product-artwork/synthetic-screen.png \
  --screen-portrait scripts/product-artwork/synthetic-screen-portrait.png
python scripts/encode-product-r45.py --source /path/to/portable-r45 --assets rendered-assets
```

The encoder requires Python 3.9+ and Pillow. It creates six small WebP assets and source-hash provenance. Retain the two synthetic PNG textures beside the encoded outputs when using the encoder. PNG masters are 1800 × 1584 pixels; WebPs are deliberately smaller for the embedded website Worker. The `--draft` switch produces 900 × 792 pixel quick renders. `--shots hero,side` limits rendering to named views.

The synthetic dashboards are original illustration artwork. They are not firmware screenshots, live data or evidence of portrait firmware support. CAD renders are not photographs or evidence of physical finish, complete fit, printed strength or battery qualification.

Original source geometry is CERN-OHL-P-2.0; the rendering scripts and authored presentation artwork are MIT, consistent with the public package's notices.

Import the reviewed WebPs into this Site checkout without changing firmware assets:

```sh
python3 scripts/sync-product-assets.py --case-root /path/to/portable-r45 --renders rendered-assets
```
