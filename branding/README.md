# Glimdock branding

- `glimdock-mark.svg`: the approved website's Space Grotesk lowercase **g** in a purple rounded square.
- `glimdock-logotype.svg`: dark wordmark for light surfaces.
- `glimdock-logotype-dark.svg`: light wordmark for dark surfaces.

The SVG wordmarks contain vector outlines and need no external font, image or web
request. Their lettering comes from Space Grotesk, used on the product website;
the font's copyright and SIL Open Font License are retained in
`Space-Grotesk-OFL.txt`.

The ESP32 header uses compact static alpha masks generated from those same Space
Grotesk letterforms: weight 500 for **g**, weight 700 for **glimdock.**, with the
website's spacing and colors. The complete website lockup is scaled to 55% to fit
the 104 px home touch target at 320 × 240. The rest of the UI retains Montserrat.
The product name, colors and setup-network name are defined in
`firmware/src/brand.h`. The setup portal embeds the outlined logotype. Existing
`homelab` preference keys and API paths remain compatible with previously
configured displays.

To regenerate the SVGs and checked-in firmware masks, install
`fonttools[woff]` and Pillow, then run `python3 branding/generate.py`. The bundled
`Space-Grotesk.woff2` is the same variable font used by the website and retains its
OFL license. Regeneration needs no network access.
