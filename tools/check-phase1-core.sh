#!/bin/sh
# Dependency-free core checks. This does NOT compile the application or WGSL.
set -eu
cd "$(dirname "$0")/.."
RUSTC=${RUSTC:-rustc}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
for module in bmd_baked gamma_curve scene_lights client_region_map client_region_stream client_region_shade client_regional_light client_sun light_consumers view_settings model_light model_material model_queue glow_modulation refraction_fog post_reference model_dds terrain_lightmaps gfx_light_audit gfx_color_track glow_state; do post_plan color_chain fog_matrix; do
    "$RUSTC" --edition=2021 --test "src/$module.rs" -o "$work/$module"
    "$work/$module"
done
