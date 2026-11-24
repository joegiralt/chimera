# Routes into VCA

The MOD matrix's **VCA** column (OUTPUT VCA in the readout) is the note's amp. What a route into it does depends on whether it is the first.

## With no route, the engine shapes the note

With nothing routed into VCA, each engine shapes its own level: ALGO by its carriers' envelopes, MODAL by its decay, DRUM by its pads, VA and TAPE by the key.

## The first route replaces that contour

Once any route goes into VCA, the routes are the amp. Its gain is AMP's **BASE**, plus each route's amount times its source, held between silence and full:

gain = BASE + the sum of the routes into VCA, between 0 and full

The engine still plays under that gain: ALGO's carriers, MODAL's resonator and DRUM's pads keep their own shape, scaled by it. On VA and TAPE, the routes are the note's whole contour, which is why their INIT Sounds route ENV 1 into VCA.

BASE is 0 to begin with. So a lone **PRESS** route on VCA is silent until you lean into the key or pad: that is not broken aftertouch, there is just nothing under it. In the matrix, an empty cell of the VCA column says so: `VCA: ROUTES REPLACE CONTOUR`, or `VCA: SUM STARTS AT AMP BASE` once an ENV is routed into VCA.

Pressure is linear: a tenth of the pressure gives a tenth of the route's amount.

## Swell or duck a held note, keeping its attack

On ALGO, MODAL and DRUM, set the starting point with BASE (AMP, cell E), then let PRESS add to it or take from it:

- **Duck under pressure:** BASE 127, PRESS at −64. The note plays the engine's own contour, its normal attack too, and pressing all the way halves it.
- **Swell under pressure:** BASE 64, PRESS at +63. The note plays at half, and pressing all the way brings it to full.
- **Pressure only:** BASE 0, PRESS alone. Silent until pressed.

On ALGO, MODAL and DRUM, a BASE above 0 keeps the carriers', the resonator's or the pads' own tail after key up.

On VA and TAPE, ENV 1 is already routed into VCA at +127 and is the note's contour, so leave BASE at 0: above 0 it would hold the gain up past ENV 1's attack and release. To duck, add PRESS at a negative amount. To swell, lower ENV 1's amount to leave room (+64, say) and add PRESS at a positive one: on top of a full ENV 1, PRESS can't go higher.

Until a route goes into VCA, AMP's VEL and BASE are dimmed: they have nothing to act on. BASE's focus band then says `NEEDS A ROUTE INTO VCA`. VEL scales the routes into VCA by how hard the note was played.
