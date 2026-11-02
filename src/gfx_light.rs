//! Point-light element decoding and single-keypoint execution for the actual
//! retail corpus. This is not a general GFX graph/attachment/visual player.
//! Parent transforms, active GFX state and scaled tick time are caller inputs.
use crate::{gfx_color_track::ColorTrack, scene_lights::DynamicLight};
#[derive(Debug, Clone)]
pub struct Definition {
    pub priority: i32,
    pub range: f32,
    start_ms: u32,
    duration_ms: Option<u32>,
    position: [f32; 3],
    color: u32,
    track: Option<ColorTrack>,
}
impl Definition {
    /// Interior colour-table entries whose two candidate construction
    /// precisions disagree. Reported by the corpus survey because the native
    /// loading-thread precision is not established (see `gfx_color_track`).
    pub fn construction_divergences(&self) -> usize {
        self.track
            .as_ref()
            .map_or(0, ColorTrack::construction_divergences)
    }
}
fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse()
        .map_err(|_| format!("invalid GFX light number: {s}"))
}
fn packed(s: &str) -> Result<u32, String> {
    let value: i64 = number(s)?;
    if !(-2147483648..=4294967295).contains(&value) {
        return Err("invalid GFX packed color".into());
    }
    Ok(value as u32)
}
fn unique<'a>(fields: &[(&'a str, &'a str)], key: &str) -> Result<&'a str, String> {
    let mut found = fields.iter().filter(|(k, _)| *k == key);
    let value = found
        .next()
        .ok_or_else(|| format!("missing GFX light {key}"))?
        .1;
    if found.next().is_some() {
        return Err(format!("duplicate GFX light {key}"));
    }
    Ok(value)
}
fn take<'a>(fields: &'a [(&'a str, &'a str)], i: &mut usize, key: &str) -> Result<&'a str, String> {
    let (k, v) = *fields.get(*i).ok_or("truncated GFX light keypoint")?;
    if k != key {
        return Err(format!("expected GFX light {key}, got {k}"));
    }
    *i += 1;
    Ok(v)
}
fn vec3(s: &str) -> Result<[f32; 3], String> {
    let v = s
        .split(',')
        .map(|v| number::<f32>(v.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let v: [f32; 3] = v.try_into().map_err(|_| "invalid GFX light position")?;
    if v.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite GFX position".into());
    }
    Ok(v)
}
fn decode(fields: &[(&str, &str)]) -> Result<Definition, String> {
    let split = fields
        .iter()
        .position(|(k, _)| *k == "KEYPOINTCOUNT")
        .ok_or("missing GFX keypoint count")?;
    let common = &fields[..split];
    if unique(common, "LightType")? != "1"
        || unique(common, "RepeatCount")? != "1"
        || unique(common, "RepeatDelay")? != "0"
        || unique(common, "BindEle")? != ""
        || unique(common, "DummyEle")? != ""
        || unique(common, "IsDummy")? != "0"
        || unique(common, "MatchGrnd")? != "0"
    {
        return Err("unsupported GFX light type/repeat/binding/ground flags".into());
    }
    for (k, v) in common {
        if matches!(*k, "GroundHeight" | "GroundNormal" | "InnerUse") && *v != "0" {
            return Err("unsupported GFX ground/inner-use light".into());
        }
    }
    let range: f32 = number(unique(common, "Range")?)?;
    if !range.is_finite() || range < 0. {
        return Err("invalid GFX light range".into());
    }
    let priority = number(unique(common, "Priority")?)?;
    let start_ms = number(unique(common, "StartTime")?)?;
    let mut i = split;
    if take(fields, &mut i, "KEYPOINTCOUNT")? != "1"
        || take(fields, &mut i, "InterpolateMode")? != "0"
    {
        return Err("unsupported GFX light keypoint layout".into());
    }
    let duration: i64 = number(take(fields, &mut i, "TimeSpan")?)?;
    let duration_ms = if duration == -1 {
        None
    } else {
        Some(u32::try_from(duration).map_err(|_| "invalid GFX light duration")?)
    };
    let position = vec3(take(fields, &mut i, "Position")?)?;
    let color = packed(take(fields, &mut i, "Color")?)?;
    let scale: f32 = number(take(fields, &mut i, "Scale")?)?;
    if !scale.is_finite() {
        return Err("nonfinite GFX keypoint scale".into());
    }
    // Point-light position uses parent TM and keypoint position, not this
    // keypoint's direction, rotation or scale. Range is not scaled by kp.scale.
    let direction = take(fields, &mut i, "Direction")?;
    let q = direction
        .split(',')
        .map(|x| number::<f32>(x.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    if q.len() != 4 || q.iter().any(|v| !v.is_finite()) {
        return Err("invalid GFX keypoint direction".into());
    }
    let rad: f32 = number(take(fields, &mut i, "Rad_2D")?)?;
    if !rad.is_finite() {
        return Err("nonfinite GFX keypoint angle".into());
    }
    let count: u32 = number(take(fields, &mut i, "CtrlMethodCount")?)?;
    let track = match count {
        0 => None,
        1 => {
            if take(fields, &mut i, "CtrlType")? != "108" {
                return Err("unsupported GFX light controller".into());
            }
            let start = number(take(fields, &mut i, "StartTime")?)?;
            let end = number(take(fields, &mut i, "EndTime")?)?;
            let original = packed(take(fields, &mut i, "Color")?)?;
            let count: usize = number(take(fields, &mut i, "Count")?)?;
            if count == 0 || count > 10 {
                return Err("invalid GFX color destination count".into());
            }
            let mut destinations = Vec::with_capacity(count);
            for _ in 0..count {
                let color = packed(take(fields, &mut i, "Color")?)?;
                let duration = number(take(fields, &mut i, "TimeSpan")?)?;
                destinations.push((color, duration));
            }
            Some(ColorTrack::new(start, end, original, &destinations)?)
        }
        _ => return Err("unsupported GFX light controller count".into()),
    };
    if i != fields.len() {
        return Err("unrecognized trailing GFX light keypoint/controller data".into());
    }
    Ok(Definition {
        priority,
        range,
        start_ms,
        duration_ms,
        position,
        color,
        track,
    })
}
/// Decode light elements only; callers still have to execute the surrounding
/// GFX graph and resolve active parent transforms. Unknown light paths fail.
pub fn parse(text: &str) -> Result<Vec<Definition>, String> {
    let mut lines = text.lines();
    let version: u32 = number(
        lines
            .next()
            .and_then(|l| l.strip_prefix("MOXTVersion: "))
            .ok_or("expected text GFX")?
            .trim(),
    )?;
    if !matches!(version, 37 | 39 | 41 | 45 | 52 | 58 | 64 | 73 | 75 | 79) {
        return Err("unverified GFX light version".into());
    }
    let mut expected = None;
    let mut elements = 0;
    let mut current = None;
    let mut fields = Vec::new();
    let mut definitions = Vec::new();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "GFXELEMENTCOUNT" {
            if expected.is_some() {
                return Err("duplicate GFX element count".into());
            }
            let n: usize = number(value)?;
            if n > 100000 {
                return Err("excessive GFX element count".into());
            }
            expected = Some(n);
        } else if key == "GFXELEMENTID" {
            elements += 1;
            if current == Some(130) {
                definitions.push(decode(&fields)?);
            }
            fields.clear();
            current = Some(number::<u32>(value)?);
        } else if current == Some(130) {
            fields.push((key, value));
        }
    }
    if current == Some(130) {
        definitions.push(decode(&fields)?);
    }
    if expected != Some(elements) {
        return Err("GFX element count mismatch".into());
    }
    Ok(definitions)
}
#[derive(Debug)]
pub struct Emitter {
    definition: Definition,
    elapsed: u32,
    color: u32,
    finished: bool,
    active: bool,
}
impl Emitter {
    pub fn new(definition: Definition) -> Self {
        let color = definition.color;
        Self {
            definition,
            elapsed: 0,
            color,
            finished: false,
            active: false,
        }
    }
    pub fn reset(&mut self) {
        self.elapsed = 0;
        self.color = self.definition.color;
        self.finished = false;
        self.active = false;
    }
    /// Input delta has already been scaled by the GFX owner's clock. Visibility
    /// and priority gate publication, not keypoint progression (0x70ba00).
    pub fn tick(
        &mut self,
        delta_ms: u32,
        visible: bool,
        manager_priority: i32,
        parent: [f32; 16],
        outer: Option<[f32; 4]>,
    ) -> Result<Option<DynamicLight>, String> {
        if parent.iter().any(|v| !v.is_finite())
            || outer.is_some_and(|v| v.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err("invalid GFX light parent/outer color".into());
        }
        if self.finished {
            return Ok(None);
        }
        self.elapsed = self.elapsed.wrapping_add(delta_ms);
        if self.elapsed >= self.definition.start_ms {
            self.active = true;
            let ms = self.elapsed - self.definition.start_ms;
            if self.definition.duration_ms.is_some_and(|d| ms >= d) {
                self.finished = true;
                return Ok(None);
            }
            if let Some(track) = &self.definition.track {
                if let Some(color) = track.sample(ms as f32 / 1000.) {
                    self.color = color;
                }
            }
        } else if !self.active {
            return Ok(None);
        }
        // DWORD wrap before StartTime leaves a previously active keypoint's
        // process data intact in 0x6edd80; it does not unpublish that live key.
        if !visible || self.definition.priority > manager_priority {
            return Ok(None);
        }
        let mut color = [16, 8, 0, 24].map(|shift| ((self.color >> shift) & 255) as f32 / 255.);
        if let Some(outer) = outer {
            for i in 0..4 {
                color[i] =
                    (((color[i] * outer[i]) as f64 * 255.).min(255.).trunc() as u32) as f32 / 255.;
            }
        }
        let p = self.definition.position;
        let position = std::array::from_fn(|i| {
            parent[i] * p[0] + parent[i + 4] * p[1] + parent[i + 8] * p[2] + parent[i + 12]
        });
        if position.iter().any(|v: &f32| !v.is_finite()) {
            return Err("nonfinite GFX world light position".into());
        }
        Ok(Some(DynamicLight {
            position,
            radius: self.definition.range,
            color,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDENTITY: [f32; 16] = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    fn file() -> String {
        "MOXTVersion: 37\nGFXELEMENTCOUNT: 1\nGFXELEMENTID: 130\nRepeatCount: 1\nRepeatDelay: 0\nBindEle: \nDummyEle: \nIsDummy: 0\nMatchGrnd: 0\nLightType: 1\nPriority: 4\nRange: 10\nStartTime: 100\nKEYPOINTCOUNT: 1\nInterpolateMode: 0\nTimeSpan: 1000\nPosition: 1, 2, 3\nColor: -2139062144\nScale: 7\nDirection: 0, 0, 0, 1\nRad_2D: 0\nCtrlMethodCount: 0\n".into()
    }
    #[test]
    fn actual_decoding_start_end_parent_transform_and_scale_semantics() {
        let def = parse(&file()).unwrap().pop().unwrap();
        let mut emitter = Emitter::new(def);
        assert!(emitter.tick(99, true, 4, IDENTITY, None).unwrap().is_none());
        let mut parent = IDENTITY;
        parent[0] = 2.;
        parent[5] = 3.;
        parent[10] = 4.;
        parent[12] = 10.;
        let light = emitter.tick(1, true, 4, parent, None).unwrap().unwrap();
        assert_eq!(light.position, [12., 6., 12.]);
        assert_eq!(light.radius, 10.); // neither parent nor kp scale changes range
        assert_eq!(light.color, [128. / 255.; 4]);
        assert!(emitter.tick(999, true, 4, parent, None).unwrap().is_some());
        assert!(emitter.tick(1, true, 4, parent, None).unwrap().is_none());
        assert!(emitter.tick(0, true, 4, parent, None).unwrap().is_none());
        emitter.reset();
        assert!(emitter.tick(100, true, 4, parent, None).unwrap().is_some());
    }
    #[test]
    fn hidden_and_low_priority_lights_still_progress_before_publication_gate() {
        let mut e = Emitter::new(parse(&file()).unwrap().pop().unwrap());
        assert!(e.tick(100, false, 4, IDENTITY, None).unwrap().is_none());
        assert!(e.tick(500, true, 3, IDENTITY, None).unwrap().is_none());
        assert!(e.tick(500, true, 4, IDENTITY, None).unwrap().is_none());
        let infinite = file().replace("TimeSpan: 1000", "TimeSpan: -1");
        let mut e = Emitter::new(parse(&infinite).unwrap().pop().unwrap());
        let light = e
            .tick(100, true, 4, IDENTITY, Some([0.5, 1., 0., 0.5]))
            .unwrap()
            .unwrap();
        assert_eq!(light.color, [64. / 255., 128. / 255., 0., 64. / 255.]);
    }
    #[test]
    fn unsupported_or_corrupt_light_layouts_are_errors_not_fake_lights() {
        for (a, b) in [
            ("RepeatCount: 1", "RepeatCount: 2"),
            ("KEYPOINTCOUNT: 1", "KEYPOINTCOUNT: 2"),
            ("CtrlMethodCount: 0", "CtrlMethodCount: 2"),
            ("LightType: 1", "LightType: 2"),
            ("Range: 10", "Range: NaN"),
            ("GFXELEMENTCOUNT: 1", "GFXELEMENTCOUNT: 2"),
            ("BindEle: ", "BindEle: parent"),
            ("Version: 37", "Version: 999"),
        ] {
            assert!(parse(&file().replace(a, b)).is_err(), "{a}");
        }
    }
    #[test]
    fn controller_108_is_applied_before_packed_outer_color() {
        let source=file().replace("StartTime: 100","StartTime: 0").replace("TimeSpan: 1000","TimeSpan: -1").replace("CtrlMethodCount: 0", "CtrlMethodCount: 1\nCtrlType: 108\nStartTime: 0\nEndTime: -1\nColor: 0\nCount: 1\nColor: 2139062143\nTimeSpan: 1000");
        let mut e = Emitter::new(parse(&source).unwrap().pop().unwrap());
        // PC24 table index 63 holds byte 63, not the extended-precision 62.
        assert_eq!(
            e.tick(500, true, 4, IDENTITY, None).unwrap().unwrap().color,
            [63. / 255.; 4]
        );
        assert_eq!(
            e.tick(500, true, 4, IDENTITY, None).unwrap().unwrap().color,
            [127. / 255.; 4]
        );
    }
    #[test]
    fn infinite_key_preserves_active_state_across_owner_dword_wrap() {
        let source = file().replace("TimeSpan: 1000", "TimeSpan: -1");
        let mut e = Emitter::new(parse(&source).unwrap().pop().unwrap());
        assert!(e.tick(100, true, 4, IDENTITY, None).unwrap().is_some());
        e.elapsed = u32::MAX;
        assert!(e.tick(1, true, 4, IDENTITY, None).unwrap().is_some());
        e.reset();
        assert!(e.tick(0, true, 4, IDENTITY, None).unwrap().is_none());
    }
}
