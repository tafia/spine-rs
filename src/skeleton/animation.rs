//! Module to interpolate animated sprites

use skeleton;
use skeleton::error::SkeletonError;

/// Struct to handle animated skin and calculate sprites
pub struct SkinAnimation<'a> {
    skeleton: &'a skeleton::Skeleton,
    animation: Option<&'a skeleton::Animation>,
    // skin: &'a skeleton::Skin,
    // default_skin: &'a skeleton::Skin,
    
    // attachments as defined by skin name (or default skin) ordered by slot
    // it is possible not to find an attachment for a slot on setup pose 
    // as it may be set during the animation
    skin_attachments: Vec<Option<&'a skeleton::Attachment>>
    duration: f32
}

/// Interpolated slot with attachment and color
pub struct Sprite {
    /// attachment name
    pub attachment: String,
    /// scale, rotate, translate
    pub srt: skeleton::SRT,
    /// color
    pub color: Vec<u8>,
    /// positions
    pub positions: [(f32, f32); 4]
}

impl<'a> SkinAnimation<'a> {

    /// Iterator<Item=Vec<CalculatedSlot>> where item are modified with timelines
    pub fn new(skeleton: &'a skeleton::Skeleton, skin: &str, animation: Option<&str>)
        -> Result<SkinAnimation<'a>, SkeletonError>
    {
        // search all attachments defined by the skin name (use 'default' skin if not found)
        let skin = try!(skeleton.skins.get(skin)
            .ok_or(SkeletonError::SkinNotFound(skin.into())));
        let default_skin = try!(skeleton.skins.get("default")
            .ok_or(SkeletonError::SkinNotFound("default".into())));
        let skin_attachments = self.skeleton.slots.iter().enumerate().map(|(i, slot)| {
            slot.attachment.as_ref().and_then(|slot_attach|
                self.skin.find(i, &slot_attach)
                .or_else(|| self.default_skin.find(i, &slot_attach)))
        }.collect();

        // get animation
        let (animation, duration) = if let Some(animation) = animation {
            let anim = try!(skeleton.animations.get(animation)
                .ok_or(SkeletonError::AnimationNotFound(animation.into())));
            (Some(anim), anim.duration)
        } else {
            (None, 0f32)
        };

        Ok(SkinAnimation {
            skeleton: skeleton,
            animation: animation,
            // skin: skin,
            // default_skin: default_skin,
            duration: duration,
            skin_attachments: skin_attachments,
        })
    }

    /// Interpolates animated slots at given time
    pub fn interpolate(&self, time: f32) -> Option<Vec<Sprite>> {

        if time > self.duration {
            return None;
        }

        // get all bones srt
        let mut srts: Vec<skeleton::SRT> = Vec::with_capacity(self.skeleton.bones.len());
        for (i, b) in self.skeleton.bones.iter().enumerate() {

            // starts with default bone srt
            let mut srt = b.srt.clone();

            // parent srt: translate bone (do not inherit scale and rotation yet)
            if let Some(ref parent_srt) = b.parent_index.and_then(|p| srts.get(p)) {
                srt.position.0 += parent_srt.position.0;
                srt.position.1 += parent_srt.position.1;
            }

            // animation srt
            if let Some(anim_srt) = self.animation
                .and_then(|anim| anim.bones.iter().find(|&&(idx, _)| idx == i ))
                .map(|&(_, ref anim)| anim.srt(time)) {
                srt.add_assign(&anim_srt);
            }

            srts.push(srt)
        }

        // loop all slots and animate them
        let mut result = Vec::new();
        for (i, slot) in self.skeleton.slots.iter().enumerate() {
            
            // TODO: change attachment if animating
            
            // nothing to show if there is no attachment
            if let Some(ref skin_attach) = self.skin_attachments[i] {

                let mut srt = srts[slot.bone_index].clone();
                srt.add_assign(&skin_attach.srt);

                // color
                let color = self.animation
                    .and_then(|anim| anim.slots.iter()
                        .find(|&&(idx, _)| idx == i )
                        .map(|&(_, ref anim)| (*anim).interpolate_color(time)))
                    .unwrap_or(vec![255, 255, 255, 255]);

                let attach_name = skin_attach.name.clone().or_else(|| slot.attachment.clone())
                    .expect("no attachment name provided");

                result.push(Sprite {
                    attachment: attach_name,
                    srt: srt,
                    color: color
                });
            }
        }

        Some(result)
    }

    /// Creates an iterator which iterates slots at delta seconds interval
    pub fn iter(&'a self, delta: f32) -> AnimationIter<'a> {
        AnimationIter {
            skin_animation: &self,
            time: 0f32,
            delta: delta
        }
    }
}

/// Iterator over a constant period
pub struct AnimationIter<'a> {
    skin_animation: &'a SkinAnimation<'a>,
    time: f32,
    delta: f32
}

impl<'a> Iterator for AnimationIter<'a> {
    type Item = Vec<Sprite>;
    fn next(&mut self) -> Option<Vec<Sprite>> {
        let result = self.skin_animation.interpolate(self.time);
        self.time += self.delta;
        result
    }
}
