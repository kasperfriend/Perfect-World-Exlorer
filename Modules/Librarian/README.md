# Librarian

Librarian browses the Perfect World 1.4.2 v27 `elements.data` installed in the
selected client directory. Enable it from **Escape → Modules → Librarian**.
It reads client archives without modifying them; missing optional archives do
not prevent the item index from opening.

Select a category and type, search by name or ID, and select a result for its
icon, decoded attributes, available add-on pools, and linked equipment sets.
Add items to the Comparison tab to align their known numeric attributes. The
first column in each category is its baseline; arrows reorder columns within the
category, × removes a column, and
the item name returns to its details. Add-on weights are the authored pool
weights, not necessarily final in-game probabilities. Comparison columns have
explicit Left, Right and Remove controls.

Switch to **3D model** for an offscreen ECM/SMD preview, or a class/gender
floating equipment mesh for armor and fashion. Select the class and gender
variant used to load the equipment mesh; no character body is displayed. Drag to orbit, Shift-drag to pan, and scroll to
zoom, or use the Distance slider; turntable and animation controls are below the
image. Dyeable fashion has an approximate color-tint picker, not an exact dye
mask simulation. Chinese action names use the client’s data/fzl2jw.ttf or
data/fzxh1jw.ttf if available; otherwise actions are numbered. Preview models and
textures require the corresponding client archives or matching loose files. Missing resources and
unsupported models are reported in the preview instead of being substituted.
For effect-only models with an embedded GFX skin model, the preview can show
the first embedded model in bind pose (requires `gfx.pck`). It omits GFX
transforms, animation, particles, decals and other effects; effect-only models
without a supported embedded model cannot be previewed.
Skeletons with unsupported reflection or constraint controllers use an
explicitly labelled approximate pose.

Client files and localized data vary. Unknown item fields and add-on types are
not guessed: unrecognized add-ons are shown as raw values. The comparison shows
only attributes currently decoded for each item type, and does not calculate a
character's final combat stats or apply randomized add-ons.

For optional tests against an installed client, set `PW_LIBRARIAN_ELEMENTS` to
its `data/elements.data` path and `PW_LIBRARIAN_ELEMENT_ROOT` to its element
folder before running `cargo test --locked --bin perfect-world-explorer`.
Set `PW_LIBRARIAN_GPU=1` as well to render real weapon and worn-armor previews
and read back their pixels on a headless GPU.

The supplied `pw142` client indexes 13,483 unique items across 43 nonempty
item tables, exceeding the 12,922-item reference. Counts may differ between
client builds; the browser does not discard extra records to force a count.
