//! Skeleton structs
//! Owns json::Animation

pub mod error;
mod timelines;
pub mod animation;

use json;
use from_json;
use std::collections::HashMap;
use std::io::Read;
use std::f32::consts::PI;
use std::f32::{cos, sin};
use serialize::hex::FromHex;

// Reexport skeleton modules
use self::error::SkeletonError;
use self::timelines::{BoneTimeline, SlotTimeline};
use self::animation::SkinAnimation;

const TO_RADIAN: f32 = PI / 180f32;

fn bone_index(name: &str, bones: &[Bone]) -> Result<usize, SkeletonError> {
    bones.iter().position(|b| b.name == *name).ok_or(SkeletonError::BoneNotFound(name.into()))
}

fn slot_index(name: &str, slots: &[Slot]) -> Result<usize, SkeletonError> {
    slots.iter().position(|b| b.name == *name).ok_or(SkeletonError::SlotNotFound(name.into()))
}

/// Skeleton data converted from json and loaded into memory
pub struct Skeleton {
    /// bones for the skeleton, hierarchically ordered
    bones: Vec<Bone>,
    /// slots
    slots: Vec<Slot>,
    /// skins : key: skin name, value: slots attachments
    skins: HashMap<String, Skin>,
    /// all the animations
    animations: HashMap<String, Animation>
}

impl Skeleton {

    /// Consumes reader (with json data) and returns a skeleton wrapping
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Skeleton, SkeletonError> {

        // read and convert as json
        let document = try!(from_json::Json::from_reader(&mut reader));
        let document: json::Document = try!(from_json::FromJson::from_json(&document));

        // convert to skeleton (consumes document)
        Skeleton::from_json(document)
    }

    /// Creates a from_json skeleton
    /// Consumes json::Document
    fn from_json(doc: json::Document) -> Result<Skeleton, SkeletonError> {

        let mut bones = Vec::new();
        if let Some(jbones) = doc.bones {
            for b in jbones.into_iter() {
                let bone = try!(Bone::from_json(b, &bones));
                bones.push(bone);
            }
        }

        let mut slots = Vec::new();
        if let Some(jslots) = doc.slots {
            for s in jslots.into_iter() {
                let slot = try!(Slot::from_json(s, &bones));
                slots.push(slot);
            }
        }

        let mut animations = HashMap::new();
        for janimations in doc.animations.into_iter() {
            for (name, animation) in janimations.into_iter() {
                let animation = try!(Animation::from_json(animation, &bones, &slots));
                animations.insert(name, animation);
            }
        }

        let mut skins = HashMap::new();
        for jskin in doc.skins.into_iter() {
            for (name, jslots) in jskin.into_iter() {
                let mut skin = Vec::new();
                for (name, attachments) in jslots.into_iter() {
                    let slot_index = try!(slot_index(&name, &slots));
                    let attachments = attachments.into_iter().map(|(name, attachment)| {
                        (name, Attachment::from_json(attachment))
                     }).collect();
                    skin.push((slot_index, attachments));
                }
                skins.insert(name, Skin {
                    slots: skin
                });
            }
        }

        Ok(Skeleton {
            bones: bones,
            slots: slots,
            skins: skins,
            animations: animations
        })
    }

    /// Gets a SkinAnimation which can interpolate slots at a given time
    pub fn get_animated_skin<'a>(&'a self, skin: &str, animation: Option<&str>)
        -> Result<SkinAnimation<'a>, SkeletonError>
    {
        SkinAnimation::new(self, skin, animation)
    }
}

/// Skin
/// defines a set of slot with custom attachments
/// slots: Vec<(slot_index, HashMap<custom_attachment_name, Attachment>)>
/// TODO: simpler architecture
struct Skin {
    /// all slots modified by the skin, the default skin contains all skeleton bones
    slots: Vec<(usize, HashMap<String, Attachment>)>
}

impl Skin {
    /// find attachment in a skin
    fn find(&self, slot_index: usize, attach_name: &str) -> Option<&Attachment> {
        self.slots.iter().filter_map(|&(i, ref attachs)|
            if i == slot_index {
                attachs.get(attach_name)
            } else {
                None
            }).next()
    }
}

/// Animation with precomputed data
struct Animation {
    bones: Vec<(usize, BoneTimeline)>,
    slots: Vec<(usize, SlotTimeline)>,
    events: Vec<json::EventKeyframe>,
    draworder: Vec<json::DrawOrderTimeline>,
    duration: f32
}

impl Animation {

    /// Creates a from_json Animation
    fn from_json(animation: json::Animation, bones: &[Bone], slots: &[Slot])
        -> Result<Animation, SkeletonError>
    {
        let duration = Animation::duration(&animation);

        let mut abones = Vec::new();
        for jbones in animation.bones.into_iter() {
            for (name, timelines) in jbones.into_iter() {
                let index = try!(bone_index(&name, bones));
                let timeline = try!(BoneTimeline::from_json(timelines));
                abones.push((index, timeline));
            }
        }

        let mut aslots = Vec::new();
        for jslots in animation.slots.into_iter() {
            for (name, timelines) in jslots.into_iter() {
                let index = try!(slot_index(&name, slots));
                let timeline = try!(SlotTimeline::from_json(timelines));
                aslots.push((index, timeline));
            }
        }

        Ok(Animation {
            // data: animation,
            duration: duration,
            bones: abones,
            slots: aslots,
            events: animation.events.unwrap_or(Vec::new()),
            draworder: animation.draworder.unwrap_or(Vec::new()),
        })
    }

    fn duration(animation: &json::Animation) -> f32 {
        animation.bones.iter().flat_map(|bones| bones.values().flat_map(|timelines|{
            timelines.translate.iter().flat_map(|translate| translate.iter().map(|e| e.time))
            .chain(timelines.rotate.iter().flat_map(|rotate| rotate.iter().map(|e| e.time)))
            .chain(timelines.scale.iter().flat_map(|scale| scale.iter().map(|e| e.time)))
        }))
        .chain(animation.slots.iter().flat_map(|slots| slots.values().flat_map(|timelines|{
            timelines.attachment.iter().flat_map(|attachment| attachment.iter().map(|e| e.time))
            .chain(timelines.color.iter().flat_map(|color| color.iter().map(|e| e.time)))
        })))
        .fold(0.0f32, f32::max)
    }
}

/// Scale, Rotate, Translate struct
#[derive(Debug, Clone)]
pub struct SRT {
    /// scale
    pub scale: (f32, f32),
    /// rotation
    pub rotation: f32,
    /// position or translation
    pub position: (f32, f32),
}

impl SRT {
    /// add assign other srt to current srt
    pub fn add_assign(&mut self, other: &SRT) {
        self.position.0 += other.position.0;
        self.position.1 += other.position.1;
        self.rotation += other.rotation;
        self.scale.0 *= other.scale.0;
        self.scale.1 *= other.scale.1;
    }
}

/// skeleton bone
struct Bone {
    name: String,
    parent_index: Option<usize>,
    length: f32,
    srt: SRT
}

impl Bone {
    fn from_json(bone: json::Bone, bones: &[Bone]) -> Result<Bone, SkeletonError> {
        let index = match bone.parent {
            Some(ref name) => Some(try!(bone_index(name, bones))),
            None => None
        };
        Ok(Bone {
            name: bone.name,
            parent_index: index,
            length: bone.length.unwrap_or(0f32),
            srt: SRT {
                scale: (bone.scaleX.unwrap_or(1f32), bone.scaleY.unwrap_or(1f32)),
                rotation: bone.rotation.unwrap_or(0f32) * TO_RADIAN,
                position: (bone.x.unwrap_or(0f32), bone.y.unwrap_or(0f32)),
            }
        })
    }
}

/// skeleton slot
struct Slot {
    name: String,
    bone_index: usize,
    color: Vec<u8>,
    attachment: Option<String>
}

impl Slot {
    fn from_json(slot: json::Slot, bones: &[Bone]) -> Result<Slot, SkeletonError> {
        let bone_index = try!(bone_index(&slot.bone, &bones));
        let color = try!(slot.color.unwrap_or("FFFFFFFF".into()).from_hex());
        Ok(Slot {
            name: slot.name,
            bone_index: bone_index,
            color: color,
            attachment: slot.attachment
        })
    }
}

/// skeletom animation
struct Attachment {
    name: Option<String>,
    type_: json::AttachmentType,
    srt: SRT,
    size: (f32, f32),
    fps: Option<f32>,
    mode: Option<String>,
    //vertices: Option<Vec<??>>     // TODO: ?
}

impl Attachment {
    fn from_json(attachment: json::Attachment) -> Attachment {
        Attachment {
            name: attachment.name,
            type_: attachment.type_.unwrap_or(json::AttachmentType::Region),
            srt: SRT {
                scale: (attachment.scaleX.unwrap_or(1f32), attachment.scaleY.unwrap_or(1f32)),
                rotation: attachment.rotation.unwrap_or(0f32) * TO_RADIAN,
                position: (attachment.x.unwrap_or(0f32), attachment.y.unwrap_or(0f32)),
            },
            size: (attachment.width.unwrap_or(0f32), attachment.height.unwrap_or(0f32)),
            fps: attachment.fps,
            mode: attachment.mode
        }
    }
    
    /// gets 4 positions defining the transformed attachment
    fn get_positions(&self, srt: &SRT) -> [[f32; 2]; 4] {
        // compute final transformations
        let srt = self.srt.clone().add_assign(srt);
        // scaled half-sizes (shape is initially centered)
        let (w2, h2) = (self.size.0 / 2.0 * srt.scale.0, self.size.1 / 2.0 * srt.scale.1);
        // rotation matrix
        let (cos, sin) = (srt.rotation.cos(), srt.rotation.sin());
        let rotation = [[cos, -sin], [sin, cos]];
        // apply rotation then translation on each vector
        [rotate_translate!([-w2,  y2], rotation, srt.position),
         rotate_translate!([ w2,  y2], rotation, srt.position),
         rotate_translate!([ w2, -y2], rotation, srt.position),
         rotate_translate!([-w2, -y2], rotation, srt.position)]
    }
}

macro_rules! rotate_translate {
    ($v: expr, $rotation: expr, $translate: expr) => {
        [$rotation[0][0] * $v[0] + $rotation[0][1] * $v[1] + $translate.0, 
         $rotation[1][0] * $v[0] + $rotation[1][1] * $v[1] + $translate.1]
    };
}
