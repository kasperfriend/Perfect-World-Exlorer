//! Character-specific pose adapter. Ordinary models keep using the validated
//! core evaluator. Player skeletons additionally contain fake/flipped bones and
//! controller-only joints; their authored transforms remain in place while
//! class-15 animation joints consume STCK tracks. No core pose rules are changed.

use crate::{animation::BoneTrack, skeleton::Skeleton, skeleton_pose::Pose};
use glam::{Mat4, Quat, Vec3};
use std::collections::VecDeque;

pub fn evaluate(
    skeleton: &Skeleton,
    tracks: &[BoneTrack],
    seconds: f32,
    model: Mat4,
) -> Result<Pose, String> {
    if skeleton.bones.iter().all(|b| b.flags == 0) && skeleton.joints.iter().all(|j| j.class == 15)
    {
        return crate::skeleton_pose::evaluate(skeleton, tracks, seconds, model);
    }
    if !seconds.is_finite() || !model.is_finite() {
        return Err("nonfinite character pose input".into());
    }
    if skeleton.bones.iter().any(|b| b.flags & !0x03 != 0) {
        return Err("unknown character bone flags".into());
    }
    let count = skeleton.bones.len();
    let mut local: Vec<_> = skeleton
        .bones
        .iter()
        .map(|b| Mat4::from_cols_array(&b.relative))
        .collect();
    let mut assigned = vec![false; count];
    for track in tracks {
        let joint = usize::try_from(track.id)
            .ok()
            .and_then(|i| skeleton.joints.get(i))
            .ok_or("character track references an invalid joint")?;
        if joint.class != 15 {
            // Controlled/spring joints are driven by their controllers in the
            // client, not by an animation track. Walker retains their authored
            // default rather than misreading a controller as a rotation.
            continue;
        }
        let bone = usize::try_from(joint.child)
            .ok()
            .filter(|&i| i < count)
            .ok_or("character joint references an invalid child bone")?;
        if assigned[bone] {
            return Err("multiple character joints target one bone".into());
        }
        assigned[bone] = true;
        let position = track
            .position
            .sample(seconds)
            .ok_or("missing/nonfinite character position track")?;
        let rotation = track
            .rotation
            .sample(seconds)
            .ok_or("missing/nonfinite character rotation track")?;
        local[bone] =
            Mat4::from_rotation_translation(Quat::from_array(rotation), Vec3::from_array(position));
    }

    let mut children = vec![Vec::new(); count];
    let mut queue = VecDeque::new();
    for (index, bone) in skeleton.bones.iter().enumerate() {
        if bone.parent == -1 {
            queue.push_back(index);
        } else {
            let parent = usize::try_from(bone.parent)
                .ok()
                .filter(|&i| i < count && i != index)
                .ok_or("invalid character pose hierarchy")?;
            children[parent].push(index);
        }
        if !local[index].is_finite() || !Mat4::from_cols_array(&bone.initial).is_finite() {
            return Err("nonfinite character bone matrix".into());
        }
        if bone.flags & 0x02 != 0 {
            // A3DBone::BuildFrameUpToRootMatrix negates _11, _12, _13.
            local[index].x_axis.x = -local[index].x_axis.x;
            local[index].y_axis.x = -local[index].y_axis.x;
            local[index].z_axis.x = -local[index].z_axis.x;
        }
    }

    let mut absolute = vec![Mat4::IDENTITY; count];
    let mut skin = vec![Mat4::IDENTITY; count];
    let mut visited = 0;
    while let Some(i) = queue.pop_front() {
        let parent = skeleton.bones[i].parent;
        absolute[i] = if parent < 0 {
            model * local[i]
        } else {
            absolute[parent as usize] * local[i]
        };
        skin[i] = absolute[i] * Mat4::from_cols_array(&skeleton.bones[i].initial);
        if !absolute[i].is_finite() || !skin[i].is_finite() {
            return Err("nonfinite composed character pose".into());
        }
        queue.extend(children[i].iter().copied());
        visited += 1;
    }
    if visited != count {
        return Err("cyclic character pose hierarchy".into());
    }
    Ok(Pose {
        absolute,
        skin,
        model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        animation::KeyTrack,
        skeleton::{Bone, Joint},
    };

    fn skeleton(class: u32) -> Skeleton {
        Skeleton {
            version: 6,
            start: 0,
            end: 1,
            fps: 30,
            bones: vec![Bone {
                name: "Bip01 Head".into(),
                flags: 0,
                parent: -1,
                first_joint: 0,
                relative: Mat4::from_translation(Vec3::new(0., 1.7, 0.)).to_cols_array(),
                initial: Mat4::IDENTITY.to_cols_array(),
                children: vec![],
            }],
            joints: vec![Joint {
                class,
                name: "head".into(),
                parent: -1,
                child: 0,
                sibling: -1,
            }],
            hooks: vec![],
            embedded_tracks: vec![],
        }
    }

    fn track() -> BoneTrack {
        BoneTrack {
            id: 0,
            position: KeyTrack {
                fps: 30,
                keys: vec![[9., 9., 9.]],
                frames: vec![0.],
                length_ms: 0,
                compression: 0,
            },
            rotation: KeyTrack {
                fps: 30,
                keys: vec![[0., 0., 0., 1.]],
                frames: vec![0.],
                length_ms: 0,
                compression: 0,
            },
        }
    }

    #[test]
    fn absolute_head_matrix_includes_character_world_transform() {
        let pose = evaluate(
            &skeleton(15),
            &[],
            0.,
            Mat4::from_translation(Vec3::new(4., 2., -3.)),
        )
        .unwrap();
        assert_eq!(
            pose.absolute[0].transform_point3(Vec3::ZERO),
            Vec3::new(4., 3.7, -3.)
        );
    }

    #[test]
    fn controller_only_joint_keeps_authored_default_instead_of_track() {
        let pose = evaluate(&skeleton(12), &[track()], 0., Mat4::IDENTITY).unwrap();
        assert_eq!(
            pose.absolute[0].transform_point3(Vec3::ZERO),
            Vec3::new(0., 1.7, 0.)
        );
    }
}
