#!/bin/sh
# Dependency-free core checks. This does NOT compile the application or WGSL.
set -eu
cd "$(dirname "$0")/.."
RUSTC=${RUSTC:-rustc}
root=$(pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM

for module in bmd_baked gamma_curve scene_lights client_region_map client_region_stream client_region_shade client_regional_light client_sun light_consumers view_settings post_plan color_chain fog_matrix model_light model_material model_queue model_receiver_class terrain_sampling terrain_block dxt glow_modulation refraction_fog post_reference model_dds terrain_lightmaps gfx_light_audit gfx_color_track glow_state; do
    "$RUSTC" --edition=2021 --test "src/$module.rs" -o "$work/$module"
    "$work/$module"
done
# Multi-module harness for the effect owner lifecycle. Written as a synthetic
# crate root because gfx_owner consumes gfx_light and scene_lights.
cat > "$work/gfx_owner_harness.rs" <<EOF
#[path = "$root/src/gfx_color_track.rs"]
mod gfx_color_track;
#[path = "$root/src/scene_lights.rs"]
mod scene_lights;
#[path = "$root/src/gfx_light.rs"]
mod gfx_light;
#[path = "$root/src/gfx_owner.rs"]
mod gfx_owner;
EOF
"$RUSTC" --edition=2021 --test "$work/gfx_owner_harness.rs" -o "$work/gfx_owner_harness"
"$work/gfx_owner_harness"
