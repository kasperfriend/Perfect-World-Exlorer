# Live logging: how to test, and how to report what you saw

Everything here is in **Settings ▸ LOGGING**, the fifth tab. It is off by default
and does nothing until a box is ticked, so it is safe to leave alone.

## Where the files go

`<explorer folder>/pwe-logs/` — next to the viewer's executable, **not** inside
the client install.

| How you start the viewer | Where the logs land |
| --- | --- |
| Shipped build, `Perfect-World-Explorer.exe` in `D:\PWE\` | `D:\PWE\pwe-logs\` |
| `cargo run` / `target\debug\…` | the checkout root, `target\debug` is unwrapped |
| Old settings that pointed inside the client | moved to the explorer root on first start |

The page shows the resolved path and what is in it (`7 files, 2.4 MB`) under the
folder box. **Open** shows it in the file manager, **Explorer folder** puts the
box back to the default, and **Clear files** deletes the `.log` files so the next
capture starts from an empty folder.

Two kinds of file:

* **`<channel>.log`** — one per ticked channel, `[+12.345]` on every line, rotated
  to `.log.1` at 8 MiB;
* **`session.log`** — always written: the run header (client root, explorer root,
  version, mask), every capture started, every folder change, every **F8** mark.
  It is the file that explains the others a week later.

## One click per report

The **CAPTURES** row ticks exactly the set a known report needs and leaves
everything else off. A capture replaces the mask instead of adding to it, so a
file is about one question.

| Button | For |
| --- | --- |
| **Sun rays** | `树林光束` — direction, scale, texture, blend of every shaft |
| **Archosaur beams** | `天煞光束` — the two horizontal shafts and their timing |
| **Waterfall** | `水门_瀑布` — UV window against the clock that should advance it |
| **Crater colour** | the saturated disc: what the map placed, what the fold produced |
| **Trails and rings** | Wraithgate's ring, ribbons, waves, fractals |
| **Everything** | all 36 channels, for a report that has not been narrowed down |

Channels are grouped under headings that say what the line is *for* — **Where it
is**, **Cards**, **Sub-elements**, **Diagnosis** — and each one has a tooltip
naming its fields. **All on this page** / **None on this page** and **Nothing**
cover the bulk cases.

## F8 marks the spot

Press **F8** anywhere in the world and a line is written to `session.log` and to
every channel that is on, whatever the throttle says:

```
---- MARK camera (1528.0, 602.5, 1032.3) yaw 0.000 pitch -0.150 time 12.00 ----
```

Press it once when the artefact comes into view and once before you leave. The
pair of marks is the window to read; everything between them is the evidence,
with a position and a timestamp instead of a description.

## The five steps

1. **Click the capture** for the report.
2. **Fly to the spot, get the artefact on screen, press F8.**
3. **Wait a few seconds** — two or three frames of a waterfall, one full turn of
   a ring — then **press F8 again**.
4. **Copy report.** It carries the build, the client root, the folder, every
   switch that is on, what each of the 36 channels wrote (lines and throttled
   lines), and the live tail. That block plus one sentence about what is wrong is
   a complete report.
5. **Attach the two or three files the report names** if the tail is not enough —
   not the folder. `Clear files` between defects.

(`Copy tail` copies the live tail alone, when the header is already in the
report.)

## Reading a channel that produced nothing

| What you see | What it means |
| --- | --- |
| *no hook in this build yet* next to the box | there is no call site for it yet: ticking it writes nothing, on purpose |
| `on, nothing yet` | wired, ticked, and genuinely silent so far — it has not been reached |
| a count that climbs, an empty tail | the tab is scrolled away, or the lines rolled off; the file has them |
| `N throttled` | the channel limits itself per millisecond; the lines were dropped on purpose, not lost to a bug |
| the file exists but is empty | the header was written and nothing followed — same as `on, nothing yet` |

## What each open report needs

| Report | Capture | What settles it |
| --- | --- | --- |
| Sun rays point at random | **Sun rays** | `effect-frames` (keypoint + basis columns) and `effect-cards` (corners) against the `dir`/`up` pair `scene-effects` prints for the same placement |
| Archosaur beams are both horizontal | **Archosaur beams** | `effect-frames` for both bases; `effect-clock` for whether the second placement starts late |
| Waterfall does not move | **Waterfall** | `effect-uv` against `effect-clock`: window advancing with the clock, or pinned while it runs |
| Crater is pure blue | **Crater colour** | `scene-effects` names the placement, `effect-cards` the root, `effect-textures` whether the texture resolved, `effect-alpha` the fold |
| Wraithgate ring spins fully | **Trails and rings** | `effect-trails` for the ribbon, `effect-uv` for the scroll, `effect-clock` for the turn |
| Shroom lights sit too low | **Sun rays** | `effect-cards` for each card's world Y, `effect-ground` for whether a drape moved it |

## If a capture is impossible

Say so instead of waiting: a channel marked *no hook in this build yet* cannot
produce a file, and a report that asks for one is a report that cannot be
answered. The rest of the header — build, client root, mask, folder — is still
worth pasting, because it rules out the wrong build and the wrong folder first.
