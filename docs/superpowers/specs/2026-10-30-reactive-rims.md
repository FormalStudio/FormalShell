# Reactive rims

Owner, 2026-10-30: the cards' rims react, slightly and in real time, to what
is around them, the way Liquid Glass edges do. Light from the wallpaper and
from the windows next to a card tints and brightens its 1px border and lays a
thin specular line inside it. Subtle: the rim is still a 1px line of about
the border's own weight, never a glow. Behind `theme.reactiveRims`
(default true). Wins over DESIGN.md's flat-border wording where the two
disagree; the table's border stays the rim's base colour.

Retuned the same night (owner: "more like an outline than an edge, too
intense, and it doesn't impact the rest of the surface"): the rim is a lit
edge of a physical card. Its light peaks at the point nearest its source
and fades along the edge, and a fraction of it spills onto the fill. The
fill always moves clearly less than the border.

## 1. What a rim reads

The capture rule holds: nothing here captures a window or the output.

- **The wallpaper.** The shell draws it, so it already holds the pixels:
  each output's cover-cropped `Picture`. One downsampled copy per picture
  (`fs_chrome::rim::Sky`, a grid of 8 logical pixels to a cell, box
  averaged) is made when the picture changes and kept until it does again.
  A flat colour wallpaper is a one-cell sky.
- **The windows, as geometry.** The compositor model's rects on the card's
  output (the active workspace's windows, fullscreen and hidden ones left
  out), in output coordinates, and Hyprland's `general:col.active_border`
  (read with the blur option, on connect and on `configreloaded`). The
  focused window glows in that colour at full strength; any other window
  in neutral white at under half of it. Mango has no border colour to
  read and gets the neutral glow for every window.

Hyprland raises no event for a window moved by pixels, so while a
transient reactive card is up (a panel, a popout, a modal, the OSD, a
toast) the shell asks for the model again every 250ms
(`Command::RefreshWindows`). The answer is compared before it is published,
so an unchanged one wakes no surface. Nothing polls with no such card up;
the sidebar's cards never close, so they follow the model's own events
alone, and a window moved by pixels beside them lights them at the next one.

## 2. What a rim draws

For each of 24 stops round the card, at even angles from its centre: the
point on the card's edge along that ray and the edge's outward normal there.

- **Tint**: the sky averaged just outside and just under that point. The
  stop's border colour is the table's border mixed 20% toward the tint.
- **Lift**: the sky's luminance there (up to 0.25), at full weight only on
  the stretch over the sky's brightest part and at 0.3 of it where the sky
  is no brighter than the rim's mean; plus each window's light: facing the
  stop (the normal against the direction to the window's nearest point),
  within 64px and falling off with the square of the distance, and along
  the edge a Gaussian 72px wide round the point of the card nearest the
  window (the middle of the stretch the two share). Each stop then takes a
  quarter of each neighbour's lift, so the light fades along the edge.
  Capped at 1. The border brightens toward white by 0.28 of the lift, and
  its alpha rises by at most 0.15 of what it lacks.
- **Specular**: a 1px line one pixel inside the border, white mixed 30%
  toward the tint, at 0.16 of the lift. Zero lift draws nothing.
- **Spill**: the same sweep filled over the card's fill at 0.12 of the lift,
  whitened by the lift, under the content, weighted by a falloff mask that
  is 1 at the edge and 0 from 36px in (quadratic, from the nearest edge).
  The mask is made once per fill size at a quarter of its logical
  resolution and stretched, so a changing light only recomposes it.
- **Wash**: the sky's mean tint flat over the whole fill at 0.03. Flat
  because a sweep carried to the centre draws its seams there.

Stops are quantised (colour to 1/255, lift to 1/64), so a sky or a window
that did not really change gives the same stops and damages nothing.

The rim lines are one sweep gradient stroke each, the spill one masked fill, drawn by vello_cpu the way
every other paint is (`Paint::Rim`, the gradient held in surface pixels so
a joined card's deform and edge map leave it in place). They replace the
flat border stroke; the border is never drawn twice. The GPU path draws
the same paint.

## 3. Motion and cost

A change of stops eases from the old ones to the new on `spatialFast`, off
the frame clock (`motion.rs`), fractional, at the output's rate. A card at
rest whose stops did not change asks for no frame and commits nothing; the
`--idle` and `--menu-budget` budgets hold unchanged. The stops are computed
when the card's rest rect, its output's sky or the window model changes,
never per frame: 24 points, a few sky reads each, one pass over the
windows.

## 4. Where

Every card: the panel host (panels, the notification centre), popouts and
the tray menu, the launcher, the OSD, toasts, and the sidebar's cards. A
surface not on that list (the bar strip, tooltips, the lock) keeps its flat
border, and so does a modal's popover card under `motion.morph: false`,
whose look the compositor draws off viewport-cropped quads.

## 5. Verify

`--reactive-rims`: a wallpaper red on its left half and blue on its right,
a panel open over each half in turn, the rim's pixels read off the frame:
red over blue on the left card, blue over red on the right. A floating
window moved next to the open card brightens the facing rim (`debug dump`'s
`rims` and the frame), read along the stretch the window spans. The fill
10px inside the lit edge is brighter than 10px inside the far one, and
moved by less than the rim. Five seconds at rest with the card open: no commit
on its surface. `theme.reactiveRims: false` retargeted under the running
shell puts the flat border back.
