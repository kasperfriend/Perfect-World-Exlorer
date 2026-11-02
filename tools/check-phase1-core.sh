#!/bin/sh
# Dependency-free core checks. This does NOT compile the application or WGSL.
set -eu
cd "$(dirname "$0")/.."
RUSTC=${RUSTC:-rustc}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
for module in gamma_curve scene_lights client_region_map client_sun view_settings model_light model_material model_queue glow_modulation refraction_fog post_reference model_dds; do
    "$RUSTC" --edition=2021 --test "src/$module.rs" -o "$work/$module"
    "$work/$module"
done
