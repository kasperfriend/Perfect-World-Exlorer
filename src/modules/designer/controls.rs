//! Egui controls for every stored native customization and equipment field.

use super::{
    catalog::{ArmorSlot, Catalog},
    customize::{CharacterDesign, SliderBounds},
};

pub(super) fn show(
    ui: &mut egui::Ui,
    design: &mut CharacterDesign,
    catalog: &Catalog,
    bounds: &SliderBounds,
) -> bool {
    let mut changed = false;
    macro_rules! slider {
        ($section:literal, $key:literal, $field:ident, $label:literal) => {{
            let range = bounds.range($section, $key);
            let mut value = design.customize.face.$field as i32;
            if ui.add(egui::Slider::new(&mut value, range).text($label)).changed() {
                design.customize.face.$field = value.clamp(0, 255) as u8;
                changed = true;
            }
        }};
    }
    macro_rules! body_slider {
        ($key:literal, $field:ident, $label:literal) => {{
            let range = bounds.range("Body", $key);
            let mut value = design.customize.$field as i32;
            if ui.add(egui::Slider::new(&mut value, range).text($label)).changed() {
                design.customize.$field = value.clamp(0, 255) as u8;
                changed = true;
            }
        }};
    }
    macro_rules! id {
        ($field:ident, $label:literal) => {{
            if ui
                .add(egui::DragValue::new(&mut design.customize.face.$field).speed(1))
                .changed()
            {
                changed = true;
            }
            ui.label($label);
        }};
    }

    egui::CollapsingHeader::new("Body and proportions")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Body variant ID");
                if ui
                    .add(egui::DragValue::new(&mut design.customize.body_id).speed(1))
                    .changed()
                {
                    changed = true;
                }
            });
            changed |= color(ui, "Body color (ARGB)", &mut design.customize.color_body);
            body_slider!("headScale", head_scale, "Head scale");
            body_slider!("upScale", up_scale, "Upper torso");
            body_slider!("waistScale", waist_scale, "Waist");
            body_slider!("armWidth", arm_width, "Arm width");
            body_slider!("legWidth", leg_width, "Leg width");
            body_slider!("breastScale", breast_scale, "Breast scale");
            slider!("3Parts", "scaleUp", scale_up, "Face upper third");
            slider!("3Parts", "scaleMiddle", scale_middle, "Face middle third");
            slider!("3Parts", "scaleDown", scale_down, "Face lower third");
            slider!("Face", "scaleFaceH", scale_face_h, "Face width");
            slider!("Face", "scaleFaceV", scale_face_v, "Face height");
            changed |= color(ui, "Face color", &mut design.customize.face.color_face);
        });

    egui::CollapsingHeader::new("Face shape and complexion").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 0, "Face shape 1", 0);
        changed |= shape_combo(ui, catalog, design, 0, "Face shape 2", 1);
        slider!("BlendFace", "blendFaceShape", blend_face_shape, "Shape blend");
        changed |= texture_combo(ui, catalog, design, 0, "Face texture", FaceId::Face);
        ui.horizontal(|ui| {
            id!(id_third_eye, "Third-eye GFX ID");
        });
    });

    egui::CollapsingHeader::new("Forehead, temples and cheeks").show(ui, |ui| {
        slider!("Forehead", "offsetForeheadH", offset_forehead_h, "Forehead horizontal");
        slider!("Forehead", "offsetForeheadV", offset_forehead_v, "Forehead vertical");
        slider!("Forehead", "offsetForeheadZ", offset_forehead_z, "Forehead depth");
        slider!("Forehead", "rotateForehead", rotate_forehead, "Forehead rotation");
        slider!("Forehead", "scaleForehead", scale_forehead, "Forehead scale");
        slider!("YokeBone", "offsetYokeBoneH", offset_yoke_bone_h, "Temple horizontal");
        slider!("YokeBone", "offsetYokeBoneV", offset_yoke_bone_v, "Temple vertical");
        slider!("YokeBone", "offsetYokeBoneZ", offset_yoke_bone_z, "Temple depth");
        slider!("YokeBone", "rotateYokeBone", rotate_yoke_bone, "Temple rotation");
        slider!("YokeBone", "scaleYokeBone", scale_yoke_bone, "Temple scale");
        slider!("Cheek", "offsetCheekH", offset_cheek_h, "Cheek horizontal");
        slider!("Cheek", "offsetCheekV", offset_cheek_v, "Cheek vertical");
        slider!("Cheek", "offsetCheekZ", offset_cheek_z, "Cheek depth");
        slider!("Cheek", "scaleCheek", scale_cheek, "Cheek scale");
    });

    egui::CollapsingHeader::new("Chin and jaw").show(ui, |ui| {
        slider!("Chain", "offsetChainV", offset_chain_v, "Chin vertical");
        slider!("Chain", "offsetChainZ", offset_chain_z, "Chin depth");
        slider!("Chain", "rotateChain", rotate_chain, "Chin rotation");
        slider!("Chain", "scaleChainH", scale_chain_h, "Chin width");
        slider!("Jaw", "offsetJawH", offset_jaw_h, "Jaw horizontal");
        slider!("Jaw", "offsetJawV", offset_jaw_v, "Jaw vertical");
        slider!("Jaw", "offsetJawZ", offset_jaw_z, "Jaw depth");
        slider!("Jaw", "scaleJawSpecial", scale_jaw_special, "Jaw special scale");
        slider!("Jaw", "scaleJawH", scale_jaw_h, "Jaw width");
        slider!("Jaw", "scaleJawV", scale_jaw_v, "Jaw height");
    });

    egui::CollapsingHeader::new("Eyes and eyelids").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 1, "Eye shape", 2);
        changed |= texture_combo(ui, catalog, design, 1, "Eye base texture", FaceId::EyeBase);
        changed |= texture_combo(ui, catalog, design, 1, "Eye highlight texture", FaceId::EyeHigh);
        changed |= texture_combo(ui, catalog, design, 6, "Eyeball texture", FaceId::Eyeball);
        changed |= color(ui, "Eyelid color", &mut design.customize.face.color_eye);
        changed |= color(ui, "Eyeball color", &mut design.customize.face.color_eye_ball);
        ui.label("Right eye / first native controller set");
        slider!("Eye", "scaleEyeH", scale_eye_h, "Width");
        slider!("Eye", "scaleEyeV", scale_eye_v, "Height");
        slider!("Eye", "rotateEye", rotate_eye, "Rotation");
        slider!("Eye", "offsetEyeH", offset_eye_h, "Horizontal offset");
        slider!("Eye", "offsetEyeV", offset_eye_v, "Vertical offset");
        slider!("Eye", "offseteyeZ", offset_eye_z, "Depth");
        slider!("Eye", "scaleEyeBall", scale_eye_ball, "Eyeball scale");
        ui.label("Left eye / second native controller set");
        slider!("Eye", "scaleEyeH2", scale_eye_h2, "Width 2");
        slider!("Eye", "scaleEyeV2", scale_eye_v2, "Height 2");
        slider!("Eye", "rotateEye2", rotate_eye2, "Rotation 2");
        slider!("Eye", "offsetEyeH2", offset_eye_h2, "Horizontal offset 2");
        slider!("Eye", "offsetEyeV2", offset_eye_v2, "Vertical offset 2");
        slider!("Eye", "offseteyeZ2", offset_eye_z2, "Depth 2");
        slider!("Eye", "scaleEyeBall2", scale_eye_ball2, "Eyeball scale 2");
    });

    egui::CollapsingHeader::new("Eyebrows").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 2, "Brow shape", 3);
        changed |= texture_combo(ui, catalog, design, 3, "Brow texture", FaceId::Brow);
        changed |= color(ui, "Brow color", &mut design.customize.face.color_brow);
        slider!("Brow", "scaleBrowH", scale_brow_h, "Width");
        slider!("Brow", "scaleBrowV", scale_brow_v, "Height");
        slider!("Brow", "rotateBrow", rotate_brow, "Rotation");
        slider!("Brow", "offsetBrowH", offset_brow_h, "Horizontal offset");
        slider!("Brow", "offsetBrowV", offset_brow_v, "Vertical offset");
        slider!("Brow", "offsetBrowZ", offset_brow_z, "Depth");
        slider!("Brow", "scaleBrowH2", scale_brow_h2, "Width 2");
        slider!("Brow", "scaleBrowV2", scale_brow_v2, "Height 2");
        slider!("Brow", "rotateBrow2", rotate_brow2, "Rotation 2");
        slider!("Brow", "offsetBrowH2", offset_brow_h2, "Horizontal offset 2");
        slider!("Brow", "offsetBrowV2", offset_brow_v2, "Vertical offset 2");
        slider!("Brow", "offsetBrowZ2", offset_brow_z2, "Depth 2");
    });

    egui::CollapsingHeader::new("Nose").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 3, "Nose tip shape", 4);
        changed |= shape_combo(ui, catalog, design, 4, "Nose bridge shape", 5);
        changed |= texture_combo(ui, catalog, design, 5, "Nose texture", FaceId::Nose);
        slider!("Nose", "scaleNoseTipH", scale_nose_tip_h, "Tip width");
        slider!("Nose", "scaleNoseTipV", scale_nose_tip_v, "Tip height");
        slider!("Nose", "scaleNoseTipZ", scale_nose_tip_z, "Tip depth");
        slider!("Nose", "offsetNoseTipV", offset_nose_tip_v, "Tip vertical offset");
        slider!("Nose", "scaleBridgeTipH", scale_bridge_tip_h, "Bridge width");
        slider!("Nose", "OffsetBridgeTipZ", offset_bridge_tip_z, "Bridge depth");
    });

    egui::CollapsingHeader::new("Mouth").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 5, "Upper lip line", 6);
        changed |= shape_combo(ui, catalog, design, 6, "Middle lip line", 7);
        changed |= shape_combo(ui, catalog, design, 7, "Lower lip line", 8);
        changed |= texture_combo(ui, catalog, design, 4, "Mouth texture", FaceId::Mouth);
        changed |= color(ui, "Mouth color", &mut design.customize.face.color_mouth);
        slider!("Mouth", "thickUpLip", thick_up_lip, "Upper lip thickness");
        slider!("Mouth", "thickDownLip", thick_down_lip, "Lower lip thickness");
        slider!("Mouth", "scaleMouthH", scale_mouth_h, "Width");
        slider!("Mouth", "offsetMouthV", offset_mouth_v, "Vertical offset");
        slider!("Mouth", "offsetMOuthZ", offset_mouth_z, "Depth");
        slider!("Mouth", "offsetCornerOfMouthSpecial", offset_corner_of_mouth_special, "Corner offset");
        slider!("Mouth", "scaleMouthH2", scale_mouth_h2, "Width 2");
        slider!("Mouth", "offsetCornerOfMouthSpecial2", offset_corner_of_mouth_special2, "Corner offset 2");
    });

    egui::CollapsingHeader::new("Ears, hair and facial hair").show(ui, |ui| {
        changed |= shape_combo(ui, catalog, design, 8, "Ear shape", 9);
        slider!("Ear", "scaleEar", scale_ear, "Ear scale");
        slider!("Ear", "offsetEarV", offset_ear_v, "Ear vertical offset");
        changed |= hair_combo(ui, catalog, design);
        changed |= texture_combo(ui, catalog, design, 8, "Hair texture", FaceId::Hair);
        changed |= color(ui, "Hair color", &mut design.customize.face.color_hair);
        ui.horizontal(|ui| {
            id!(id_moustache_skin, "Goatee skin ID");
        });
        changed |= texture_combo(ui, catalog, design, 9, "Goatee texture", FaceId::Goatee);
        ui.horizontal(|ui| {
            id!(id_faling_skin, "Faling skin ID");
        });
        changed |= texture_combo(ui, catalog, design, 10, "Faling texture", FaceId::Faling);
        changed |= texture_combo(ui, catalog, design, 7, "Moustache texture", FaceId::Moustache);
        changed |= color(ui, "Facial-hair color", &mut design.customize.face.color_moustache);
    });

    egui::CollapsingHeader::new("Armor, fashion and weapons").show(ui, |ui| {
        if ui.checkbox(&mut design.equipment.fashion_mode, "Use fashion slots").changed() {
            changed = true;
        }
        changed |= weapon_combo(ui, catalog, design);
        for (slot, label) in [
            (ArmorSlot::Head, "Head"),
            (ArmorSlot::Upper, "Upper body"),
            (ArmorSlot::Wrist, "Wrist"),
            (ArmorSlot::Lower, "Lower body"),
            (ArmorSlot::Foot, "Foot"),
        ] {
            changed |= armor_combo(ui, catalog, design, slot, label);
            changed |= fashion_combo(ui, catalog, design, slot, label);
        }
    });
    changed
}

#[derive(Clone, Copy)]
enum FaceId {
    Face,
    EyeBase,
    EyeHigh,
    Eyeball,
    Brow,
    Nose,
    Mouth,
    Hair,
    Goatee,
    Faling,
    Moustache,
}

fn texture_combo(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    design: &mut CharacterDesign,
    part: u32,
    label: &str,
    field: FaceId,
) -> bool {
    let class = design.class;
    let gender = design.gender;
    let face = &mut design.customize.face;
    let id = match field {
        FaceId::Face => &mut face.id_face_tex,
        FaceId::EyeBase => &mut face.id_eye_base_tex,
        FaceId::EyeHigh => &mut face.id_eye_high_tex,
        FaceId::Eyeball => &mut face.id_eye_ball_tex,
        FaceId::Brow => &mut face.id_brow_tex,
        FaceId::Nose => &mut face.id_nose_tex,
        FaceId::Mouth => &mut face.id_mouth_tex,
        FaceId::Hair => &mut face.id_hair_tex,
        FaceId::Goatee => &mut face.id_goatee_tex,
        FaceId::Faling => &mut face.id_faling_tex,
        FaceId::Moustache => &mut face.id_moustache_tex,
    };
    let old = *id;
    let current = catalog.face_texture(old as u32);
    let selected = current.map_or_else(|| format!("{old} (not in v27)"), |item| format!("{} — {}", item.id, item.name));
    egui::ComboBox::from_label(label)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for item in catalog.texture_options(class, gender, part) {
                ui.selectable_value(id, item.id as u16, format!("{} — {}", item.id, item.name));
            }
        });
    old != *id
}

fn shape_combo(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    design: &mut CharacterDesign,
    part: u32,
    label: &str,
    which: usize,
) -> bool {
    let class = design.class;
    let gender = design.gender;
    let face = &mut design.customize.face;
    let id = match which {
        0 => &mut face.id_face_shape1,
        1 => &mut face.id_face_shape2,
        2 => &mut face.id_eye_shape,
        3 => &mut face.id_brow_shape,
        4 => &mut face.id_nose_tip_shape,
        5 => &mut face.id_nose_bridge_shape,
        6 => &mut face.id_mouth_up_lip_line,
        7 => &mut face.id_mouth_mid_lip_line,
        8 => &mut face.id_mouth_down_lip_line,
        _ => &mut face.id_ear_shape,
    };
    let old = *id;
    let selected = catalog.face_shape(old as u32).map_or_else(
        || format!("{old} (not in v27)"),
        |item| format!("{} — {}", item.id, item.name),
    );
    egui::ComboBox::from_label(label)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for item in catalog.shape_options(class, gender, part) {
                ui.selectable_value(id, item.id as u16, format!("{} — {}", item.id, item.name));
            }
        });
    old != *id
}

fn hair_combo(ui: &mut egui::Ui, catalog: &Catalog, design: &mut CharacterDesign) -> bool {
    let class = design.class;
    let gender = design.gender;
    let id = &mut design.customize.face.id_hair_model;
    let old = *id;
    let selected = catalog.hair_item(old as u32).map_or_else(
        || format!("{old} (not in v27)"),
        |item| format!("{} — {}", item.id, item.name),
    );
    egui::ComboBox::from_label("Hair model")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(id, 0, "0 — bald / no additional hair skin");
            for item in catalog.hair_options(class, gender) {
                ui.selectable_value(id, item.id as u16, format!("{} — {}", item.id, item.name));
            }
        });
    old != *id
}

fn weapon_combo(ui: &mut egui::Ui, catalog: &Catalog, design: &mut CharacterDesign) -> bool {
    let class = design.class;
    let id = &mut design.equipment.weapon;
    let old = *id;
    let selected = catalog.weapon(old).map_or_else(
        || format!("{old} — unarmed / not in v27"),
        |item| format!("{} — {}", item.id, item.name),
    );
    egui::ComboBox::from_label("Weapon")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(id, 0, "0 — unarmed");
            for item in catalog
                .weapons
                .iter()
                .filter(|item| item.class_mask & class.bit() != 0)
            {
                ui.selectable_value(id, item.id, format!("{} — {}", item.id, item.name));
            }
        });
    old != *id
}

fn armor_combo(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    design: &mut CharacterDesign,
    slot: ArmorSlot,
    label: &str,
) -> bool {
    let class = design.class;
    let old = design.equipment.armor(slot);
    let mut id = old;
    let selected = catalog.armor_item(old).map_or_else(
        || format!("{old} — native default / not in v27"),
        |item| format!("{} — {}", item.id, item.name),
    );
    egui::ComboBox::from_label(format!("{label} armor"))
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut id, 0, "0 — profession default");
            for item in catalog.armor.iter().filter(|item| {
                item.slot == slot && item.class_mask & class.bit() != 0
            }) {
                ui.selectable_value(&mut id, item.id, format!("{} — {}", item.id, item.name));
            }
        });
    design.equipment.set_armor(slot, id);
    old != id
}

fn fashion_combo(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    design: &mut CharacterDesign,
    slot: ArmorSlot,
    label: &str,
) -> bool {
    let gender = design.gender;
    let old = design.equipment.fashion(slot);
    let mut id = old;
    let selected = catalog.fashion_item(old).map_or_else(
        || format!("{old} — native default / not in v27"),
        |item| format!("{} — {}", item.id, item.name),
    );
    egui::ComboBox::from_label(format!("{label} fashion"))
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut id, 0, "0 — native fashion default");
            for item in catalog.fashion.iter().filter(|item| {
                item.slot == slot && item.gender == gender as u32
            }) {
                ui.selectable_value(&mut id, item.id, format!("{} — {}", item.id, item.name));
            }
        });
    design.equipment.set_fashion(slot, id);
    old != id
}

fn color(ui: &mut egui::Ui, label: &str, argb: &mut u32) -> bool {
    let mut rgba = [
        ((*argb >> 16) & 255) as f32 / 255.,
        ((*argb >> 8) & 255) as f32 / 255.,
        (*argb & 255) as f32 / 255.,
        ((*argb >> 24) & 255) as f32 / 255.,
    ];
    let changed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.color_edit_button_rgba_unmultiplied(&mut rgba)
        })
        .inner
        .changed();
    if changed {
        let c = rgba.map(|value| (value.clamp(0., 1.) * 255.).round() as u32);
        *argb = (c[3] << 24) | (c[0] << 16) | (c[1] << 8) | c[2];
    }
    changed
}

