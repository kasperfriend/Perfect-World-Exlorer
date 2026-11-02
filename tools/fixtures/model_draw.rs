use crate::{
    model_asset::{ModelAsset, PosedMesh},
    skin,
};
pub fn fixture() -> (ModelAsset, Vec<PosedMesh>) {
    let asset = ModelAsset {
        descriptor: crate::ecmodel::EcModel {
            version: 1,
            skin_model_path: "models/a/a.smd".into(),
            original_color: None,
            source_blend: None,
            destination_blend: None,
            default_speed: None,
            actions: vec![],
            events: vec![],
        },
        model: crate::ecmodel::SkinModel {
            version: 9,
            skeleton: "a.bon".into(),
            skins: vec!["a.ski".into()],
            physique: String::new(),
            track_directory: String::new(),
            actions: vec![],
            hangers: vec![],
            properties: vec![],
        },
        skeleton: crate::skeleton::Skeleton {
            version: 6,
            start: 0,
            end: 1,
            fps: 30,
            bones: vec![],
            joints: vec![],
            hooks: vec![],
            embedded_tracks: vec![],
        },
        tracks: vec![],
        skins: vec![crate::model_asset::SkinAsset {
            path: "models/a/a.ski".into(),
            data: skin::Skin {
                version: 9,
                bones: vec![],
                textures: vec!["a.dds".into()],
                materials: vec![skin::Material {
                    name: "material".into(),
                    ambient: [0.1; 4],
                    diffuse: [0.4; 4],
                    emissive: [0.; 4],
                    specular: [0., 0., 0., 1.],
                    power: 10.,
                    two_sided: true,
                }],
                meshes: vec![skin::Mesh {
                    name: "mesh".into(),
                    rigid_bone: None,
                    texture: 0,
                    material: 0,
                    indices: vec![0, 1, 2],
                    vertices: (0..3)
                        .map(|i| skin::Vertex {
                            position: [i as f32, 0., 0.],
                            normal: [0., 1., 0.],
                            uv: [i as f32 / 2., 0.],
                            weights: [1., 0., 0., 0.],
                            joints: [0; 4],
                        })
                        .collect(),
                }],
            },
        }],
    };
    let pose = PosedMesh {
        skin: 0,
        mesh: 0,
        vertices: (0..3)
            .map(|i| crate::skin_pose::PosedVertex {
                position: [i as f32, 2., 3.],
                normal: [0., 0., 1.],
            })
            .collect(),
    };
    (asset, vec![pose])
}
