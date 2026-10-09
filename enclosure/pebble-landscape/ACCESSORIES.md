# Pebble accessories

The purple blank caps supplied by the main enclosure builder leave a quiet,
flush top. Remove them and install either the bunny ears, TV antennas, or eyes.
Each pair uses the same two keyed sockets; no extra hardware or magnets are
required. These are optional ornaments, not handles for lifting the enclosure.

## Print and fit

Print the accessory fit strip and test pin first with the same PLA and profile
you will use for the case. The embossed values `0.10`, `0.15`, `0.20`, and `0.25`
are clearance **per side**, not total width increases. Choose a fit that inserts
and removes by hand without whitening the PLA or forcing the socket. The main
enclosure defaults to 0.20 mm; change its socket clearance before printing the
body if the coupon indicates another fit.

The export's `print_shape` already has the correct bed orientation. White ears
and antennae lie flat. The rounded eyes have a hidden flat back on the bed,
with their curved stalks in the same print plane and their iris recesses facing
up. Their small hidden glue pegs require support. The curved purple eye feet
also have supports enabled. Purple accents and white collars lie flat without
support. All accessory pieces stay smooth, with fuzzy skin off; the saved
projects use a brim for these small pieces.

The pins are 6 × 4 mm, 4.5 mm long for ears and antennas and 3.9 mm for eye feet,
with the local +X/+Z corner clipped by
0.8 mm. Both left and right pins share this key orientation; only the ornament
above each root is mirrored. Do not mirror the entire exported part in a slicer.
Each ear or antenna has a 10 mm wide root that spans the shallow cap recess.
The eyes emerge from separate purple feet whose visible surface follows the
case's curve. Neither attachment bottoms out in the 5 mm deep socket.

## Assembly

1. For ears and antennas, thread one thin white collar over each pin. For eyes,
   glue each white stem's 2.6 mm peg into its purple foot's 3 mm through-bore.
   The foot prints with this bore vertical. Apply glue sparingly to its walls
   and let it cure before inserting the foot into the case. Keep glue
   away from the keyed pin so the complete eye remains removable.
2. Insert the pair by hand in place of the blank caps. Pull each accessory from
   its root when removing it, rather than bending its tip.
3. If desired, glue the separate purple inner-ear pads, antenna tips, or irises
   into the open front recesses. They have 0.175 mm perimeter clearance and room
   for a thin glue film. Keep glue away from the removable keyed pins.

The visible ear is approximately 32 mm tall and 8.8 mm wide, splayed 10° outward.
The antenna arm is 41 mm long, 3.8 mm wide, and splayed 25° outward, with a 6 mm
round tip. Each eye is 12 mm across on a curved 3.4 mm stalk, extending about
22 mm above its root, with a rounded front and hidden flat rear. Its purple iris
is 4.7 mm across and 0.8 mm thick, with an open
centre above a deep blind well in the white eye that forms a shadow pupil.
The ears and antennas remain 4 mm
thick with separate 0.55 mm accents.

## Builder interface

`accessory_parts()` and `fit_coupon_parts()` return named dictionaries with
`shape`, `print_shape`, `color`, `role`, `variant`, `explode`, and assembly notes.
Installed shapes use the enclosure's local X-across/Y-up/Z-rear frame, with pin
seats at X −12/+18, Y 32, Z 16.7 mm. Values are read from `design.json`.
The main builder transforms assembly shapes to
the 18° tilted world frame, while exporting print poses directly. Use exactly
one of `ears`, `antennas`, or `eyes` for a decorated render. Include
`shared-accessory` collars only for ears and antennas. Eyes include their own
purple feet.

`validate_accessories()` checks valid single solids and a Z=0 bed pose for every
part. These checks establish nominal CAD geometry; the printed coupon determines
the actual material fit.
