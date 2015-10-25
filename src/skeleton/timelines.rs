use json;
use skeleton;
use serialize::hex::FromHex;

const BEZIER_SEGMENTS: usize = 10;

trait Interpolate {
    fn interpolate(&self, next: &Self, percent: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(&self, next: &Self, percent: f32) -> Self {
        *self + percent * (*next - *self)
    }
}

impl Interpolate for (f32, f32) {
    fn interpolate(&self, next: &Self, percent: f32) -> Self {
        (self.0 + percent * (next.0 - self.0), self.1 + percent * (next.1 - self.1))
    }
}

impl Interpolate for Vec<u8> {
    fn interpolate(&self, next: &Self, percent: f32) -> Self {
        self.iter().zip(next.iter()).map(|(&s, &n)|
            (s as f32).interpolate(&(n as f32), percent) as u8).collect()
    }
}

impl Interpolate for Option<String> {
    fn interpolate(&self, _: &Self, _: f32) -> Self {
        self.clone()
    }
}

/// Curve trait to define struct with curve property (unwrapped to Linear)
trait Curve<T> {
    fn time(&self) -> f32;
    fn curve(&self) -> json::TimelineCurve;
    fn value(&self) -> Result<T, skeleton::error::SkeletonError>;
}

/// Macro rule to implement curve based on json structs
/// The only non trivial property is the `value`
macro_rules! impl_curve {
    ($to:ty, $from:ty, $f:expr) => {
        impl Curve<$from> for $to {
            fn time(&self) -> f32 {
                self.time
            }
            fn curve(&self) -> json::TimelineCurve {
                self.curve.clone().unwrap_or(json::TimelineCurve::CurveLinear)
            }
            fn value(&self) -> Result<$from, skeleton::error::SkeletonError> {
                $f(&self)
            }
        }
    }
}

impl_curve!(json::BoneTranslateTimeline, (f32, f32), |t: &json::BoneTranslateTimeline| {
    Ok((t.x.unwrap_or(0f32), t.y.unwrap_or(0f32)))
});

impl_curve!(json::BoneScaleTimeline, (f32, f32), |t: &json::BoneScaleTimeline| {
    Ok((t.x.unwrap_or(1f32), t.y.unwrap_or(1f32)))
});

impl_curve!(json::BoneRotateTimeline, f32, |t: &json::BoneRotateTimeline| {
    let mut angle = t.angle.unwrap_or(0f32);
    while angle > 180.0 { angle -= 360.0; }
    while angle < -180.0 { angle += 360.0; }
    Ok(angle)
});

impl_curve!(json::SlotColorTimeline, Vec<u8>, |t: &json::SlotColorTimeline| {
    t.color.clone().unwrap_or("FFFFFFFF".into()).from_hex()
        .map_err(|e| skeleton::error::SkeletonError::from(e))
});

impl Curve<Option<String>> for json::SlotAttachmentTimeline {
    fn time(&self) -> f32 {
        self.time
    }
    fn curve(&self) -> json::TimelineCurve {
        json::TimelineCurve::CurveStepped
    }
    fn value(&self) -> Result<Option<String>, skeleton::error::SkeletonError> {
        Ok(self.name.clone())
    }
}

struct CurveTimeline<T> {
    time: f32,
    curve: json::TimelineCurve,
    points: Option<(Vec<f32>, Vec<f32>)>,    // bezier curve interpolations points
    value: T,
}

impl<T> CurveTimeline<T> {

    /// interpolation values (x, y)
    /// Sets the control handle positions for an interpolation bezier curve used to transition
    /// from this keyframe to the next.
    /// cx1 and cx2 are from 0 to 1, representing the percent of time between the two keyframes.
    /// cy1 and cy2 are the percent of the difference between the keyframe's values.
    fn compute_points(curve: &json::TimelineCurve) -> Option<(Vec<f32>, Vec<f32>)> {

        let (cx1, cy1, cx2, cy2) = match *curve {
            json::TimelineCurve::CurveStepped |
            json::TimelineCurve::CurveLinear  => return None, // no interpolation: early return
            json::TimelineCurve::CurveBezier(ref p)  => (p[0], p[1], p[2], p[3])
        };

        let subdiv1 = 1f32 / BEZIER_SEGMENTS as f32;
        let subdiv2 = subdiv1 * subdiv1;
        let subdiv3 = subdiv2 * subdiv1;
        let (pre1, pre2, pre4, pre5) = (3f32 * subdiv1, 3f32 * subdiv2, 6f32 * subdiv2, 6f32 * subdiv3);
        let (tmp1x, tmp1y) = (-cx1 * 2f32 + cx2, -cy1 * 2f32 + cy2);
        let (tmp2x, tmp2y) = ((cx1 - cx2) * 3f32 + 1f32, (cy1 - cy2) * 3f32 + 1f32);
        let mut dfx = cx1 * pre1 + tmp1x * pre2 + tmp2x * subdiv3;
        let mut dfy = cy1 * pre1 + tmp1y * pre2 + tmp2y * subdiv3;
        let (mut ddfx, mut ddfy) = (tmp1x * pre4 + tmp2x * pre5, tmp1y * pre4 + tmp2y * pre5);
        let (dddfx, dddfy) = (tmp2x * pre5, tmp2y * pre5);

        let (mut vec_x, mut vec_y) = (Vec::with_capacity(BEZIER_SEGMENTS), Vec::with_capacity(BEZIER_SEGMENTS));
        let (mut x, mut y) = (dfx, dfy);
        for _ in 0..BEZIER_SEGMENTS {
            vec_x.push(x);
            vec_y.push(y);
            dfx += ddfx;
            dfy += ddfy;
            ddfx += dddfx;
            ddfy += dddfy;
            x += dfx;
            y += dfy;
        }
        Some((vec_x, vec_y))
    }

    /// Get percent conversion depending on curve type
    fn get_percent(&self, percent: f32) -> f32 {


        let &(ref x,  ref y) = match self.curve {
            json::TimelineCurve::CurveStepped    => return 0f32,
            json::TimelineCurve::CurveLinear     => return percent,
            json::TimelineCurve::CurveBezier(..) => self.points.as_ref().unwrap()
        };

        // bezier curve
        match x.iter().position(|&xi| percent < xi) {
            Some(0) => y[0] * percent / x[0],
            Some(i) => y[i] + (y[i] - y[i - 1]) * (percent - x[i - 1]) / (x[i] - x[i - 1]),
            None => {
                let (x, y) = (x[BEZIER_SEGMENTS - 1], y[BEZIER_SEGMENTS - 1]);
                y + (1f32 - y) * (percent - x) / (1f32 - x)
            }
        }
    }
}

/// Set of timelines
struct CurveTimelines<T> {
    timelines: Vec<CurveTimeline<T>>
}

impl<T: Interpolate + Clone> CurveTimelines<T> {

    /// Converts vector of json timelines to vector or timelines
    fn from_json_vec<U: Curve<T>> (jtimelines: Option<Vec<U>>)
        -> Result<CurveTimelines<T>, skeleton::error::SkeletonError>
    {
    	match jtimelines {
    	    None => Ok(CurveTimelines { timelines: Vec::new() }),
    	    Some(timelines) => {
    	        let mut curves = Vec::with_capacity(timelines.len());
    	        for t in timelines.into_iter() {
    	            let value = try!(t.value());
    	            let curve = t.curve();
    	            let points = CurveTimeline::<T>::compute_points(&curve);
    	            curves.push(CurveTimeline {
    	                time: t.time(),
                        curve: curve,
                        value: value,
                        points: points
    	            });
    	        }
    	        Ok(CurveTimelines { timelines: curves })
    	    }
    	}
    }

    /// interpolates `value` in the interval containing elapsed
    fn interpolate(&self, elapsed: f32) -> Option<T> {
    	if self.timelines.len() == 0 || elapsed < self.timelines[0].time {
    	    return None;
    	}

    	if let Some(w) = self.timelines.windows(2).find(|&w| elapsed < w[1].time) {
    	    let percent = (elapsed - w[0].time) / (w[1].time - w[0].time);
    	    let curve_percent = w[0].get_percent(percent);
    	    Some(w[0].value.interpolate(&w[1].value, curve_percent))
    	} else {
    	    Some(self.timelines[self.timelines.len() - 1].value.clone())
    	}
    }
}

pub struct BoneTimeline {
    translate: CurveTimelines<(f32, f32)>,
    rotate: CurveTimelines<f32>,
    scale: CurveTimelines<(f32, f32)>,
}

impl BoneTimeline {

    /// converts json data into BoneTimeline
    pub fn from_json(json: json::BoneTimeline)
        -> Result<BoneTimeline, skeleton::error::SkeletonError>
    {
        let translate = try!(CurveTimelines::from_json_vec(json.translate));
        let rotate = try!(CurveTimelines::from_json_vec(json.rotate));
        let scale = try!(CurveTimelines::from_json_vec(json.scale));
        Ok(BoneTimeline {
            translate: translate,
            rotate: rotate,
            scale: scale,
        })
    }

    /// evaluates the interpolations for elapsed time on all timelines and
    /// returns the corresponding srt
    pub fn srt(&self, elapsed: f32) -> skeleton::SRT {
    	let (x, y) = self.translate.interpolate(elapsed).unwrap_or((0f32, 0f32));
    	let rotation = self.rotate.interpolate(elapsed).unwrap_or(0f32);
    	let (scale_x, scale_y) = self.scale.interpolate(elapsed).unwrap_or((1.0, 1.0));
    	skeleton::SRT::new(scale_x, scale_y, rotation, x, y)
    }
}

pub struct SlotTimeline {
    attachment: CurveTimelines<Option<String>>,
    color: CurveTimelines<Vec<u8>>,
}

impl SlotTimeline {
    pub fn from_json(json: json::SlotTimeline) -> Result<SlotTimeline, skeleton::error::SkeletonError> {
        let attachment = try!(CurveTimelines::from_json_vec(json.attachment));
        let color = try!(CurveTimelines::from_json_vec(json.color));
        Ok(SlotTimeline {
            attachment: attachment,
            color: color
        })
    }
    pub fn interpolate_color(&self, elapsed: f32) -> Vec<u8> {
        self.color.interpolate(elapsed).unwrap_or(vec![255, 255, 255, 255])
    }
    pub fn interpolate_attachment(&self, elapsed: f32) -> Option<Option<String>> {
        self.attachment.interpolate(elapsed)
    }
    pub fn get_attachment_names(&self) -> Vec<&str> {
        self.attachment.timelines.iter()
            .filter_map(|t| t.value.as_ref().map(|v| &**v)).collect()
    }
}
