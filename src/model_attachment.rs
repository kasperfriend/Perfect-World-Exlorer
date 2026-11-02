//! Hook attachment of ECM EventType-100 effect roots.
//!
//! Recovered 1.4.2 behaviour:
//! * `0x6f2f8c..0x6f3090` parses the event hook record in this exact order:
//!   `HookOffset: %f, %f, %f` into three floats, then `HookYaw: %f`,
//!   `HookPitch: %f`, `HookRot: %f`.
//! * `0x6e1370` turns that record into a transform: `0x7b6f10` builds a
//!   quaternion from `(HookPitch, HookYaw, HookRot)` with each angle scaled by
//!   the constant at `0x8eff98` (= 0.5, i.e. half-angles) and no degree
//!   conversion anywhere in the chain, so the record is in radians;
//!   `0x7b6ef0 -> 0x7f5711` converts it to a 3x3, and the `HookOffset` triple
//!   is copied into the matrix' last row (`+0x30` inside the matrix at
//!   `event+0x3c`), which is the client's row-vector translation slot.
//! * `0x7b2543` combines a hook with the model matrix when the hook's bind index
//!   is negative; `0x7b25a9` rejects an out-of-range bind index and `0x7b25e6`
//!   combines it with the posed bone. `0x7b2400` reads the `0x48`-byte stored
//!   hook record (name, bind index, 4x4) and `0x7b2520` resolves it per frame.
//!
//! The client stores row-vector matrices, so `hook * bone` there is
//! `bone * hook` here; an attached effect composes as `hook_world * local`.
//! Nothing in this module invents a hook: an unknown hook name, an unparsable
//! angle or a non-finite value is an explicit error.

use glam::{Mat4, Quat, Vec3};

/// Event-authored hook attachment: `HookName` plus the hook-local transform.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// `HookName` from the ECM event record.
    pub hook: String,
    /// `HookOffset` translation, applied in hook space.
    pub offset: [f32; 3],
    /// `(HookPitch, HookYaw, HookRot)` in radians, the order `0x6e1370` uses.
    pub euler: [f32; 3],
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn number(key: &str, value: &str) -> Result<f32, String> {
    let parsed: f32 = value
        .trim()
        .parse()
        .map_err(|_| format!("invalid hook {key}: {value}"))?;
    if !parsed.is_finite() {
        return Err(format!("nonfinite hook {key}"));
    }
    Ok(parsed)
}

/// `HookOffset: %f, %f, %f` — exactly three components, no defaults.
pub fn parse_offset(value: &str) -> Result<[f32; 3], String> {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(format!("invalid HookOffset: {value}"));
    }
    Ok([
        number("HookOffset x", parts[0])?,
        number("HookOffset y", parts[1])?,
        number("HookOffset z", parts[2])?,
    ])
}

/// Build the attachment from an ECM event record. `None` means the event is not
/// hook-bound; a present `HookName` requires the transform fields the native
/// record always writes (a placeholder is never substituted).
pub fn from_fields(fields: &[(String, String)]) -> Result<Option<Attachment>, String> {
    let Some(hook) = field(fields, "HookName") else {
        return Ok(None);
    };
    let hook = hook.trim();
    if hook.is_empty() {
        return Ok(None);
    }
    // The native record is always written from HookOffset/HookYaw/HookPitch/
    // HookRot; `ChildHook`/`RelHook`/`BindParent` select the target hook or the
    // relative frame and are not part of this transform.
    let offset = match field(fields, "HookOffset") {
        Some(value) => parse_offset(value)?,
        None => [0.; 3],
    };
    let mut euler = [0.; 3];
    for (index, key) in ["HookPitch", "HookYaw", "HookRot"].iter().enumerate() {
        if let Some(value) = field(fields, key) {
            euler[index] = number(key, value)?;
        }
    }
    Ok(Some(Attachment {
        hook: hook.to_string(),
        offset,
        euler,
    }))
}

impl Attachment {
    /// Hook-local transform: rotation from the half-angle quaternion of
    /// `(pitch, yaw, rot)` and the authored offset as the translation column.
    pub fn local(&self) -> Result<Mat4, String> {
        let finite = self.offset.iter().all(|v| v.is_finite());
        if !finite || self.euler.iter().any(|v| !v.is_finite()) {
            return Err(format!("nonfinite attachment for hook {:?}", self.hook));
        }
        if self.hook.is_empty() {
            return Err("attachment has an empty hook name".into());
        }
        let [pitch, yaw, rot] = self.euler;
        // 0x7b6f10: w = cx*cy*cz + sx*sy*sz with each half-angle cosine/sine.
        let (sx, cx) = ((pitch * 0.5).sin(), (pitch * 0.5).cos());
        let (sy, cy) = ((yaw * 0.5).sin(), (yaw * 0.5).cos());
        let (sz, cz) = ((rot * 0.5).sin(), (rot * 0.5).cos());
        let quat = Quat::from_xyzw(
            sx * cy * cz - cx * sy * sz,
            cx * sy * cz + sx * cy * sz,
            cx * cy * sz - sx * sy * cz,
            cx * cy * cz + sx * sy * sz,
        );
        if !quat.is_finite() {
            return Err(format!("nonfinite attachment rotation for {:?}", self.hook));
        }
        let mut matrix = quat.to_mat4();
        matrix.w_axis = Vec3::from(self.offset).extend(1.);
        Ok(matrix)
    }
}

/// Compose an attached root's world transform. `attachment` comes from this
/// module, `hook` from the parent model's posed hook (already model space), so
/// the result is `hook * attachment` in column-vector convention.
pub fn world(hook_world: Mat4, attachment: Mat4) -> Result<Mat4, String> {
    if !hook_world.is_finite() || !attachment.is_finite() {
        return Err("nonfinite attachment composition".into());
    }
    Ok(hook_world * attachment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Mat4, b: Mat4) -> bool {
        a.to_cols_array()
            .iter()
            .zip(b.to_cols_array())
            .all(|(x, y)| (x - y).abs() < 1e-6)
    }

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn absent_or_empty_hook_name_is_not_an_attachment() {
        assert_eq!(from_fields(&[]).unwrap(), None);
        let blank = fields(&[("HookName", "  ")]);
        assert_eq!(from_fields(&blank).unwrap(), None);
        let texture_only = fields(&[("FxFilePath", "a.gfx")]);
        assert_eq!(from_fields(&texture_only).unwrap(), None);
    }

    #[test]
    fn identity_attachment_leaves_the_hook_frame_untouched() {
        let attachment = from_fields(&fields(&[
            ("HookName", "weapon"),
            ("HookOffset", "0, 0, 0"),
            ("HookPitch", "0"),
            ("HookYaw", "0"),
            ("HookRot", "0"),
        ]))
        .unwrap()
        .unwrap();
        let local = attachment.local().unwrap();
        assert!(close(local, Mat4::IDENTITY));
        let hook = Mat4::from_translation(Vec3::new(1., 2., 3.));
        assert!(close(world(hook, local).unwrap(), hook));
    }

    #[test]
    fn offset_lands_in_the_translation_column() {
        let attachment = Attachment {
            hook: "h".into(),
            offset: [4., -5., 6.],
            euler: [0.; 3],
        };
        let local = attachment.local().unwrap();
        assert_eq!(local.w_axis, glam::Vec4::new(4., -5., 6., 1.));
        assert_eq!(local.x_axis, glam::Vec4::X);
        assert_eq!(local.y_axis, glam::Vec4::Y);
        assert_eq!(local.z_axis, glam::Vec4::Z);
    }

    #[test]
    fn yaw_is_a_right_handed_rotation_about_y_in_radians() {
        let attachment = Attachment {
            hook: "h".into(),
            offset: [0.; 3],
            euler: [0., std::f32::consts::FRAC_PI_2, 0.],
        };
        let local = attachment.local().unwrap();
        // Column-vector rotation: +X maps to -Z for a positive yaw.
        let rotated = local.transform_point3(Vec3::X);
        assert!(
            (rotated - Vec3::new(0., 0., -1.)).length() < 1e-6,
            "{rotated}"
        );
        assert!(local.determinant() > 0.);
    }

    #[test]
    fn pitch_and_roll_rotate_about_x_and_z() {
        let pitch = Attachment {
            hook: "h".into(),
            offset: [0.; 3],
            euler: [std::f32::consts::FRAC_PI_2, 0., 0.],
        }
        .local()
        .unwrap()
        .transform_point3(Vec3::Y);
        assert!((pitch - Vec3::Z).length() < 1e-6, "{pitch}");
        let roll = Attachment {
            hook: "h".into(),
            offset: [0.; 3],
            euler: [0., 0., std::f32::consts::FRAC_PI_2],
        }
        .local()
        .unwrap()
        .transform_point3(Vec3::X);
        assert!((roll - Vec3::Y).length() < 1e-6, "{roll}");
    }

    #[test]
    fn hook_world_is_the_parent_of_the_local_frame() {
        let local = Attachment {
            hook: "h".into(),
            offset: [0., 0., 1.],
            euler: [0.; 3],
        }
        .local()
        .unwrap();
        let hook = Mat4::from_translation(Vec3::new(10., 0., 0.));
        let hook = hook * Mat4::from_scale(Vec3::splat(2.));
        let world = world(hook, local).unwrap();
        // The offset is scaled by the hook frame, then translated by it.
        let point = world.transform_point3(Vec3::ZERO);
        assert!((point - Vec3::new(10., 0., 2.)).length() < 1e-6, "{point}");
    }

    #[test]
    fn malformed_hook_records_are_explicit_failures() {
        let short_offset = fields(&[("HookName", "h"), ("HookOffset", "1, 2")]);
        let error = from_fields(&short_offset).unwrap_err();
        assert!(error.contains("invalid HookOffset"), "{error}");
        let bad_yaw = fields(&[("HookName", "h"), ("HookYaw", "abc")]);
        let error = from_fields(&bad_yaw).unwrap_err();
        assert!(error.contains("invalid hook HookYaw"), "{error}");
        let nan_roll = fields(&[("HookName", "h"), ("HookRot", "NaN")]);
        let error = from_fields(&nan_roll).unwrap_err();
        assert!(error.contains("nonfinite hook HookRot"), "{error}");
    }

    #[test]
    fn composition_rejects_nonfinite_inputs() {
        let mut local = Mat4::IDENTITY;
        local.w_axis.x = f32::NAN;
        assert!(world(Mat4::IDENTITY, local).is_err());
        let attachment = Attachment {
            hook: String::new(),
            offset: [0.; 3],
            euler: [0.; 3],
        };
        assert!(attachment.local().is_err());
    }
}
