//! `.amd` animation metadata: per-animation stages, timing curves and bone rotations.

use super::{Cursor, Lines};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AmdFile {
    pub version: u32,
    pub groups: Vec<Group>,
    pub bone_groups: Option<Vec<BoneGroup>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub animation_type: String,
    pub animation_time: u32,
    pub animation_stages: Vec<AnimationStage>,
    pub float_group: Option<Vec<f64>>,
    pub bone_rotations: Option<Vec<BoneRotation>>,
    pub extra_ints: Option<[Option<u32>; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnimationStage {
    pub name: String,
    pub time: u32,
    pub floats: [f64; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct BoneRotation {
    pub bone: String,
    pub coord_order: String,
    pub coords: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoneGroup {
    pub name: String,
    pub bones: Vec<String>,
}

fn quoted(c: &mut Cursor) -> Option<String> {
    c.quoted().map(String::from)
}

fn uint(c: &mut Cursor) -> Option<u32> {
    c.uint()
}

fn floats(c: &mut Cursor) -> Option<Vec<f64>> {
    let n = c.uint()?;
    (0..n).map(|_| c.sp()?.float()).collect()
}

fn stage(lines: &mut Lines) -> Result<AnimationStage, String> {
    let name = lines.line("stage name", quoted)?;
    let time = lines.line("stage time", uint)?;
    let floats = lines.line("three stage floats", |c| Some([c.float()?, c.sp()?.float()?, c.sp()?.float()?]))?;
    Ok(AnimationStage { name, time, floats })
}

fn bone_rotations(lines: &mut Lines) -> Result<Vec<BoneRotation>, String> {
    let (n, per_axis) = lines.line("bone rotation count", |c| Some((c.uint()?, c.opt(|c| c.sp()?.uint()).unwrap_or(0))))?;
    lines.count(n as usize, "bone rotation", |c| {
        let bone = c.quoted()?.to_string();
        let coord_order = c.sp()?.word()?.to_string();
        let coords = (0..per_axis as usize * coord_order.len()).map(|_| c.sp()?.float()).collect::<Option<_>>()?;
        Some(BoneRotation { bone, coord_order, coords })
    })
}

fn group(lines: &mut Lines, version: u32) -> Result<Group, String> {
    let name = lines.line("quoted group name", quoted)?;
    let animation_type = lines.line("animation type", |c| c.word().map(String::from))?;
    let animation_time = lines.line("animation time", uint)?;
    let n = lines.line("stage count", uint)?;
    let animation_stages = (0..n).map(|_| stage(lines)).collect::<Result<_, _>>()?;
    let float_group = match version {
        1 => None,
        2 => lines.try_line(floats),
        _ => Some(lines.line("float group", floats)?),
    };
    let bone_rotations = if version >= 4 { Some(bone_rotations(lines)?) } else { None };
    let extra_ints = lines.attempt(|l| Some([l.try_line(|c| c.nullable_uint())?, l.try_line(|c| c.nullable_uint())?]));
    Ok(Group { name, animation_type, animation_time, animation_stages, float_group, bone_rotations, extra_ints })
}

fn bone_groups(lines: &mut Lines) -> Result<Vec<BoneGroup>, String> {
    let n = lines.line("`BoneGroups N`", |c| c.lit("BoneGroups")?.sp()?.uint())?;
    lines.count(n as usize, "bone group", |c| {
        let name = c.quoted()?.to_string();
        let n = c.sp()?.uint()?;
        let bones = (0..n).map(|_| Some(c.sp()?.quoted()?.to_string())).collect::<Option<_>>()?;
        Some(BoneGroup { name, bones })
    })
}

pub fn parse(text: &str) -> Result<AmdFile, String> {
    // Some files lose the newline before a section, leaving only the tab that indents it.
    let mut lines = Lines::split(text, &['\n', '\t'], Some("//"));
    let version = lines.version()?;
    let n = lines.line("group count", uint)?;
    let groups = (0..n).map(|_| group(&mut lines, version)).collect::<Result<_, _>>()?;
    let bone_groups = if version >= 5 { Some(bone_groups(&mut lines)?) } else { None };
    lines.end()?;
    Ok(AmdFile { version, groups, bone_groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stages_rotations_and_bone_groups() {
        let text = "version 5\r\n2\r\n\"death\"\r\n\tonce\r\n\t6166\r\n\t1\r\n\t\"remove_collidability\"\r\n\t\t150\r\n\t\t0 0 0\r\n\t0\r\n\t2 2\r\n\t\t\"L_foot\" xy 139 -59 139 -59\r\n\t\t\"R_foot\" xyz -142 86 -8 -148 93 -8\r\n\"idle\"\r\n\tloop\r\n\t1000\r\n\t0\r\n\t2 0.5 1\r\n\t0\r\n\t-1\r\n\t3\r\nBoneGroups 1\r\n\t\"legs\" 2 \"L_foot\" \"R_foot\"\r\n";
        let f = parse(text).unwrap();
        assert_eq!(f.groups.len(), 2);
        let death = &f.groups[0];
        assert_eq!(death.animation_stages[0].name, "remove_collidability");
        assert_eq!(death.animation_stages[0].time, 150);
        assert_eq!(death.float_group.as_deref(), Some(&[][..]));
        let rotations = death.bone_rotations.as_ref().unwrap();
        assert_eq!(rotations[1].coords.len(), 6);
        assert_eq!(f.groups[1].float_group.as_deref(), Some(&[0.5, 1.0][..]));
        assert_eq!(f.groups[1].extra_ints, Some([None, Some(3)]));
        assert_eq!(f.bone_groups.unwrap()[0].bones, vec!["L_foot", "R_foot"]);
    }

    #[test]
    fn version_1_has_no_float_group() {
        let f = parse("version 1\n1\n\"once\"\n\tonce\n\t1000\n\t0\n").unwrap();
        assert!(f.groups[0].float_group.is_none());
        assert!(f.bone_groups.is_none());
    }
}
