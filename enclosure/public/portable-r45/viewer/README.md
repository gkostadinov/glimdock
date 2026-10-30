# Portable r45 viewer

This static preview loads the release's exported enclosure meshes. It supports
landscape, portrait and handheld views, interchangeable accessories, a cutaway
and an exploded illustration. Click a printed component to download its STL.

Serve the release folder with an ordinary static web server and open
`viewer/index.html`; browsers generally restrict mesh loading from a `file:` URL.
The viewer has no device connection, authentication, telemetry or external font
dependency. Its local Three.js r166 module retains its MIT license in `vendor/`.

The five reference volumes in `reference/` are simple authored envelopes, not
manufacturer board CAD. The screen is synthetic demonstration artwork. The
exploded view separates components for visibility and does not specify their
installation paths. Use the release's assembly guide for the fitting sequence
and validation limits.

The HTML, CSS, JavaScript and documentation use MIT. Enclosure meshes and
illustrative geometry use CERN-OHL-P-2.0. Keep the release's license files and
adjacent asset notices with redistributed files.
