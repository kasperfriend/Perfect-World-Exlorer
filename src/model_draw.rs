//! Validated retained-asset -> ordinary drawing contract. Animation time and
//! action selection are caller inputs, never inferred from the first SMD action.
use crate::{
    model_asset::{ModelAsset, PosedMesh},
    skin,
};

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}
pub struct PreparedMesh<'a> {
    pub skin: usize,
    pub mesh: usize,
    pub vertices: Vec<Vertex>,
    pub indices: &'a [u16],
    pub material: &'a skin::Material,
    pub texture_paths: [String; 2],
}
/// Validate the complete set before any GPU uploads. Missing/duplicate poses,
/// changed topology and unsupported render overrides are errors, not permission
/// to draw a partial or incorrectly lit model.
pub fn prepare<'a>(
    asset: &'a ModelAsset,
    poses: &[PosedMesh],
) -> Result<Vec<PreparedMesh<'a>>, String> {
    let settings = asset.descriptor.render_settings();
    if settings.source_blend != 5
        || settings.destination_blend != 6
        || settings.original_color != u32::MAX
    {
        return Err("ordinary model draw does not implement this ECM color/blend override".into());
    }
    let expected: usize = asset.skins.iter().map(|s| s.data.meshes.len()).sum();
    if poses.len() != expected {
        return Err("model pose does not cover every retained mesh".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(expected);
    for pose in poses {
        if !seen.insert((pose.skin, pose.mesh)) {
            return Err("duplicate model mesh pose".into());
        }
        let skin = asset
            .skins
            .get(pose.skin)
            .ok_or("posed skin index out of range")?;
        let mesh = skin
            .data
            .meshes
            .get(pose.mesh)
            .ok_or("posed mesh index out of range")?;
        if mesh.rigid_bone.is_some() {
            return Err("ordinary weighted draw received a rigid mesh".into());
        }
        if mesh.vertices.len() != pose.vertices.len()
            || mesh.vertices.is_empty()
            || mesh.indices.is_empty()
            || mesh.indices.len() % 3 != 0
            || mesh
                .indices
                .iter()
                .any(|&i| i as usize >= mesh.vertices.len())
        {
            return Err("model pose/topology mismatch".into());
        }
        let material = usize::try_from(mesh.material)
            .ok()
            .and_then(|i| skin.data.materials.get(i))
            .ok_or("model mesh material index out of range")?;
        if material
            .ambient
            .iter()
            .chain(material.diffuse.iter())
            .chain(material.emissive.iter())
            .chain(material.specular.iter())
            .chain(std::iter::once(&material.power))
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite model material".into());
        }
        let texture = usize::try_from(mesh.texture)
            .map_err(|_| "untextured ordinary model draw is not implemented")?;
        let texture_paths = skin.texture_paths(texture)?;
        let mut vertices = Vec::with_capacity(pose.vertices.len());
        for (source, posed) in mesh.vertices.iter().zip(&pose.vertices) {
            if posed
                .position
                .iter()
                .chain(posed.normal.iter())
                .chain(source.uv.iter())
                .any(|v| !v.is_finite())
            {
                return Err("nonfinite posed model geometry/UV".into());
            }
            vertices.push(Vertex {
                position: posed.position,
                normal: posed.normal,
                uv: source.uv,
            });
        }
        result.push(PreparedMesh {
            skin: pose.skin,
            mesh: pose.mesh,
            vertices,
            indices: &mesh.indices,
            material,
            texture_paths,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_draw_fixture::fixture;
    #[test]
    fn joins_posed_geometry_to_authored_uv_material_and_texture() {
        let (asset, poses) = fixture();
        let p = prepare(&asset, &poses).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].vertices[2].position, [2., 2., 3.]);
        assert_eq!(p[0].vertices[2].normal, [0., 0., 1.]);
        assert_eq!(p[0].vertices[2].uv, [1., 0.]);
        assert_eq!(
            p[0].texture_paths,
            ["models/a/tex_a/a.dds", "models/a/textures/a.dds"]
        );
        assert!(p[0].material.two_sided);
        assert_eq!(p[0].indices, [0, 1, 2]);
    }
    #[test]
    fn rejects_incomplete_duplicate_and_changed_poses() {
        let (mut asset, mut poses) = fixture();
        assert!(prepare(&asset, &[]).is_err());
        poses[0].vertices.pop();
        assert!(prepare(&asset, &poses).is_err());
        let (a, p) = fixture();
        asset.skins[0]
            .data
            .meshes
            .push(a.skins[0].data.meshes[0].clone());
        let (_, p2) = fixture();
        let duplicates = vec![
            p.into_iter().next().unwrap(),
            p2.into_iter().next().unwrap(),
        ];
        assert!(prepare(&asset, &duplicates).is_err());
    }
    #[test]
    fn rejects_unverified_overrides_and_invalid_indices_or_floats() {
        for kind in 0..7 {
            let (mut a, mut p) = fixture();
            match kind {
                0 => a.descriptor.original_color = Some(0xff808080),
                1 => a.descriptor.destination_blend = Some(1),
                2 => a.skins[0].data.meshes[0].material = -1,
                3 => a.skins[0].data.meshes[0].texture = -1,
                4 => a.skins[0].data.meshes[0].indices[2] = 99,
                5 => p[0].vertices[0].position[0] = f32::NAN,
                _ => a.skins[0].data.meshes[0].vertices[0].uv[1] = f32::INFINITY,
            }
            assert!(prepare(&a, &p).is_err(), "invalid draw {kind}");
        }
    }
}
