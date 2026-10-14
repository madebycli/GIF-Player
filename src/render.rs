use crate::decode::Animation;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    pub fn union(&self, other: &Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            w: (self.x + self.w).max(other.x + other.w) - x,
            h: (self.y + self.h).max(other.y + other.h) - y,
        }
    }
    pub fn clip(&self, w: i32, h: i32) -> Option<Self> {
        let x = self.x.max(0);
        let y = self.y.max(0);
        let right = (self.x + self.w).min(w);
        let bottom = (self.y + self.h).min(h);
        if right > x && bottom > y {
            Some(Self {
                x,
                y,
                w: right - x,
                h: bottom - y,
            })
        } else {
            None
        }
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && x < (self.x + self.w) as f64
            && y >= self.y as f64
            && y < (self.y + self.h) as f64
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    pub rect: Rect,
    pub frame: usize,
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub editing: bool,
}
pub struct Item {
    pub id: String,
    pub animation: Arc<Animation>,
    pub signature: Signature,
}
pub type Scene = BTreeMap<String, Signature>;
pub fn scene(items: &[Item]) -> Scene {
    items
        .iter()
        .map(|i| (i.id.clone(), i.signature.clone()))
        .collect()
}
pub fn damage(before: &Scene, after: &Scene) -> Option<Rect> {
    let mut result: Option<Rect> = None;
    for (id, sig) in before {
        if after.get(id) != Some(sig) {
            result = Some(result.map_or_else(|| sig.rect.clone(), |r| r.union(&sig.rect)));
        }
    }
    for (id, sig) in after {
        if before.get(id) != Some(sig) {
            result = Some(result.map_or_else(|| sig.rect.clone(), |r| r.union(&sig.rect)));
        }
    }
    result
}
/// wl_shm ARGB8888 is a native-endian premultiplied pixel. Only damaged rows are cleared/composited.
pub fn paint(bytes: &mut [u8], width: u32, height: u32, items: &[Item], dirty: &Rect) {
    let Some(dirty) = dirty.clip(width as i32, height as i32) else {
        return;
    };
    for y in dirty.y..dirty.y + dirty.h {
        let start = (y as usize * width as usize + dirty.x as usize) * 4;
        bytes[start..start + dirty.w as usize * 4].fill(0);
    }
    for item in items {
        let sig = &item.signature;
        let rect = &sig.rect;
        let Some(intersection) = rect.clip(width as i32, height as i32).and_then(|r| {
            let x = r.x.max(dirty.x);
            let y = r.y.max(dirty.y);
            let w = (r.x + r.w).min(dirty.x + dirty.w) - x;
            let h = (r.y + r.h).min(dirty.y + dirty.h) - y;
            if w > 0 && h > 0 {
                Some(Rect { x, y, w, h })
            } else {
                None
            }
        }) else {
            continue;
        };
        let anim = &item.animation;
        let frame = &anim.frames[sig.frame.min(anim.frames.len() - 1)];
        let opacity = (sig.opacity * 255.0).round() as u16;
        let step = anim.width as usize / rect.w as usize;
        let remainder_step = anim.width as usize % rect.w as usize;
        for y in intersection.y..intersection.y + intersection.h {
            let mut sy = ((y - rect.y) as u64 * anim.height as u64 / rect.h as u64) as usize;
            if sig.flip_v {
                sy = anim.height as usize - 1 - sy;
            }
            let origin = (intersection.x - rect.x) as usize * anim.width as usize;
            let mut source_x = origin / rect.w as usize;
            let mut remainder = origin % rect.w as usize;
            for x in intersection.x..intersection.x + intersection.w {
                let mut sx = source_x;
                source_x += step;
                remainder += remainder_step;
                if remainder >= rect.w as usize {
                    source_x += 1;
                    remainder -= rect.w as usize;
                }
                if sig.flip_h {
                    sx = anim.width as usize - 1 - sx;
                }
                let src = &frame.pixels[(sy * anim.width as usize + sx) * 4..][..4];
                let alpha = (src[3] as u16 * opacity + 127) / 255;
                if alpha == 0 {
                    continue;
                }
                let offset = (y as usize * width as usize + x as usize) * 4;
                if alpha == 255 {
                    bytes[offset..offset + 4].copy_from_slice(
                        &(0xff000000u32
                            | ((src[0] as u32) << 16)
                            | ((src[1] as u32) << 8)
                            | src[2] as u32)
                            .to_ne_bytes(),
                    );
                    continue;
                }
                let old = u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
                let old_channels = [
                    ((old >> 16) & 255) as u16,
                    ((old >> 8) & 255) as u16,
                    (old & 255) as u16,
                    ((old >> 24) & 255) as u16,
                ];
                let mut out = [0u32; 4];
                for c in 0..4 {
                    out[c] = ((src[c] as u16 * opacity + 127) / 255
                        + (old_channels[c] * (255 - alpha) + 127) / 255)
                        .min(255) as u32;
                }
                bytes[offset..offset + 4].copy_from_slice(
                    &((out[3] << 24) | (out[0] << 16) | (out[1] << 8) | out[2]).to_ne_bytes(),
                );
            }
        }
        if sig.editing {
            for y in intersection.y..intersection.y + intersection.h {
                for x in intersection.x..intersection.x + intersection.w {
                    if x - rect.x < 2
                        || rect.x + rect.w - x <= 2
                        || y - rect.y < 2
                        || rect.y + rect.h - y <= 2
                    {
                        let at = (y as usize * width as usize + x as usize) * 4;
                        bytes[at..at + 4].copy_from_slice(&0xff_fff59bu32.to_ne_bytes());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Frame;
    #[test]
    fn transparency_flip_and_partial_damage() {
        let anim = Arc::new(Animation {
            width: 2,
            height: 1,
            logical_size: (2, 1),
            frames: vec![Frame {
                pixels: vec![255, 0, 0, 255, 0, 0, 128, 128].into(),
                delay_ms: 100,
            }],
            ends_ms: vec![100],
            bytes: 8,
            warning: None,
        });
        let mut items = vec![Item {
            id: "a".into(),
            animation: anim,
            signature: Signature {
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 1,
                },
                frame: 0,
                opacity: 1.0,
                flip_h: true,
                flip_v: false,
                editing: false,
            },
        }];
        let mut pixels = vec![0; 16];
        paint(
            &mut pixels,
            4,
            1,
            &items,
            &Rect {
                x: 0,
                y: 0,
                w: 4,
                h: 1,
            },
        );
        assert_eq!(
            u32::from_ne_bytes(pixels[..4].try_into().unwrap()),
            0x80000080
        );
        assert_eq!(
            u32::from_ne_bytes(pixels[4..8].try_into().unwrap()),
            0xffff0000
        );
        let before = scene(&items);
        items[0].signature.rect.x = 2;
        let after = scene(&items);
        let d = damage(&before, &after).unwrap();
        paint(&mut pixels, 4, 1, &items, &d);
        assert_eq!(&pixels[..8], &[0; 8]);
        assert!(damage(&after, &after).is_none());
    }
}
