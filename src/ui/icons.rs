//! Vector icons for the standalone interface, drawn directly with tiny-skia. They are original
//! line drawings on a 24 px grid (round caps/joins, like Noctalia's glyph style); no icon font or
//! third-party artwork is bundled.
use std::f32::consts::TAU;
use tiny_skia::{
    Color, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Search,
    Close,
    Refresh,
    ChevronRight,
    ChevronDown,
    Play,
    Pause,
    Stop,
    Move,
    Check,
    Eye,
    EyeOff,
    Trash,
    Copy,
    Settings,
    Photo,
    Sliders,
    Layers,
    Folder,
    FolderOpen,
    Save,
    Edit,
    App,
    Power,
    ArrowUpLeft,
    ArrowUpRight,
    ArrowDownLeft,
    ArrowDownRight,
    Center,
    ArrowUp,
    FlipH,
    FlipV,
    Reset,
    Activity,
    Alert,
    Info,
}
#[derive(Clone, Copy)]
enum S {
    M(f32, f32),
    L(f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
    Circle(f32, f32, f32),
    Round(f32, f32, f32, f32, f32),
}
use S::{C, Circle, L, M, Round, Z};
fn path(segments: &[S]) -> Option<Path> {
    let mut b = PathBuilder::new();
    for s in segments {
        match *s {
            M(x, y) => b.move_to(x, y),
            L(x, y) => b.line_to(x, y),
            C(a, c, d, e, f, g) => b.cubic_to(a, c, d, e, f, g),
            Z => b.close(),
            Circle(x, y, r) => b.push_circle(x, y, r),
            Round(x, y, w, h, r) => {
                let k = r * 0.5523;
                b.move_to(x + r, y);
                b.line_to(x + w - r, y);
                b.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
                b.line_to(x + w, y + h - r);
                b.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
                b.line_to(x + r, y + h);
                b.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
                b.line_to(x, y + r);
                b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
                b.close();
            }
        }
    }
    b.finish()
}
/// (stroked segments, filled segments)
fn geometry(icon: Icon) -> (Vec<S>, Vec<S>) {
    let strokes: Vec<S> = match icon {
        Icon::Search => vec![Circle(10.5, 10.5, 6.5), M(15.5, 15.5), L(20.5, 20.5)],
        Icon::Close => vec![M(6.0, 6.0), L(18.0, 18.0), M(18.0, 6.0), L(6.0, 18.0)],
        Icon::Refresh => vec![
            M(4.5, 12.0),
            C(4.5, 7.9, 7.9, 4.5, 12.0, 4.5),
            C(14.6, 4.5, 16.8, 5.8, 18.2, 7.7),
            M(19.0, 3.8),
            L(18.2, 7.7),
            L(14.3, 7.0),
            M(19.5, 12.0),
            C(19.5, 16.1, 16.1, 19.5, 12.0, 19.5),
            C(9.4, 19.5, 7.2, 18.2, 5.8, 16.3),
            M(5.0, 20.2),
            L(5.8, 16.3),
            L(9.7, 17.0),
        ],
        Icon::ChevronRight => vec![M(9.0, 6.0), L(15.0, 12.0), L(9.0, 18.0)],
        Icon::ChevronDown => vec![M(6.0, 9.0), L(12.0, 15.0), L(18.0, 9.0)],
        Icon::Play => vec![M(8.0, 5.5), L(18.5, 12.0), L(8.0, 18.5), Z],
        Icon::Pause => vec![
            Round(6.5, 5.0, 4.0, 14.0, 1.2),
            Round(13.5, 5.0, 4.0, 14.0, 1.2),
        ],
        Icon::Stop => vec![Round(6.0, 6.0, 12.0, 12.0, 2.2)],
        Icon::Move => vec![
            M(12.0, 3.0),
            L(12.0, 21.0),
            M(3.0, 12.0),
            L(21.0, 12.0),
            M(9.0, 6.0),
            L(12.0, 3.0),
            L(15.0, 6.0),
            M(9.0, 18.0),
            L(12.0, 21.0),
            L(15.0, 18.0),
            M(6.0, 9.0),
            L(3.0, 12.0),
            L(6.0, 15.0),
            M(18.0, 9.0),
            L(21.0, 12.0),
            L(18.0, 15.0),
        ],
        Icon::Check => vec![M(5.0, 12.5), L(10.0, 17.5), L(19.0, 7.0)],
        Icon::Eye | Icon::EyeOff => {
            let mut v = vec![
                M(2.5, 12.0),
                C(5.0, 7.5, 8.3, 5.5, 12.0, 5.5),
                C(15.7, 5.5, 19.0, 7.5, 21.5, 12.0),
                C(19.0, 16.5, 15.7, 18.5, 12.0, 18.5),
                C(8.3, 18.5, 5.0, 16.5, 2.5, 12.0),
                Z,
                Circle(12.0, 12.0, 3.0),
            ];
            if icon == Icon::EyeOff {
                v.extend([M(4.0, 4.0), L(20.0, 20.0)]);
            }
            v
        }
        Icon::Trash => vec![
            M(4.0, 7.0),
            L(20.0, 7.0),
            M(10.0, 11.0),
            L(10.0, 17.0),
            M(14.0, 11.0),
            L(14.0, 17.0),
            M(5.5, 7.0),
            L(6.4, 19.0),
            C(6.5, 20.1, 7.3, 21.0, 8.4, 21.0),
            L(15.6, 21.0),
            C(16.7, 21.0, 17.5, 20.1, 17.6, 19.0),
            L(18.5, 7.0),
            M(9.0, 7.0),
            L(9.0, 4.5),
            C(9.0, 3.7, 9.7, 3.0, 10.5, 3.0),
            L(13.5, 3.0),
            C(14.3, 3.0, 15.0, 3.7, 15.0, 4.5),
            L(15.0, 7.0),
        ],
        Icon::Copy => vec![
            Round(8.5, 8.5, 12.0, 12.0, 2.5),
            M(15.5, 8.5),
            L(15.5, 6.5),
            C(15.5, 5.1, 14.4, 4.0, 13.0, 4.0),
            L(6.5, 4.0),
            C(5.1, 4.0, 4.0, 5.1, 4.0, 6.5),
            L(4.0, 13.0),
            C(4.0, 14.4, 5.1, 15.5, 6.5, 15.5),
            L(8.5, 15.5),
        ],
        Icon::Settings => {
            let mut v = vec![Circle(12.0, 12.0, 3.0), Circle(12.0, 12.0, 7.0)];
            for i in 0..8 {
                let a = i as f32 / 8.0 * TAU;
                v.extend([
                    M(12.0 + a.cos() * 7.0, 12.0 + a.sin() * 7.0),
                    L(12.0 + a.cos() * 10.0, 12.0 + a.sin() * 10.0),
                ]);
            }
            v
        }
        Icon::Photo => vec![
            Round(3.5, 4.5, 17.0, 15.0, 3.0),
            Circle(9.0, 10.0, 1.6),
            M(4.0, 17.0),
            L(9.0, 12.5),
            L(13.0, 16.0),
            L(16.0, 13.0),
            L(20.0, 17.0),
        ],
        Icon::Sliders => vec![
            M(4.0, 8.0),
            L(9.5, 8.0),
            M(14.5, 8.0),
            L(20.0, 8.0),
            Circle(12.0, 8.0, 2.5),
            M(4.0, 16.0),
            L(5.5, 16.0),
            M(10.5, 16.0),
            L(20.0, 16.0),
            Circle(8.0, 16.0, 2.5),
        ],
        Icon::Layers => vec![
            M(12.0, 4.0),
            L(21.0, 9.0),
            L(12.0, 14.0),
            L(3.0, 9.0),
            Z,
            M(3.0, 13.5),
            L(12.0, 18.5),
            L(21.0, 13.5),
        ],
        Icon::Folder => vec![
            M(3.0, 7.0),
            C(3.0, 5.9, 3.9, 5.0, 5.0, 5.0),
            L(9.0, 5.0),
            L(11.5, 7.5),
            L(19.0, 7.5),
            C(20.1, 7.5, 21.0, 8.4, 21.0, 9.5),
            L(21.0, 17.0),
            C(21.0, 18.1, 20.1, 19.0, 19.0, 19.0),
            L(5.0, 19.0),
            C(3.9, 19.0, 3.0, 18.1, 3.0, 17.0),
            Z,
        ],
        Icon::FolderOpen => vec![
            M(5.0, 19.0),
            C(3.9, 19.0, 3.0, 18.1, 3.0, 17.0),
            L(3.0, 7.0),
            C(3.0, 5.9, 3.9, 5.0, 5.0, 5.0),
            L(9.0, 5.0),
            L(11.5, 7.5),
            L(18.0, 7.5),
            C(19.1, 7.5, 20.0, 8.4, 20.0, 9.5),
            L(20.0, 10.5),
            M(5.0, 19.0),
            L(7.3, 11.8),
            C(7.6, 10.9, 8.5, 10.3, 9.4, 10.3),
            L(21.5, 10.3),
            L(19.0, 17.7),
            C(18.7, 18.5, 17.9, 19.0, 17.1, 19.0),
            L(5.0, 19.0),
        ],
        Icon::Save => vec![
            M(6.0, 4.0),
            L(16.0, 4.0),
            L(20.0, 8.0),
            L(20.0, 18.0),
            C(20.0, 19.1, 19.1, 20.0, 18.0, 20.0),
            L(6.0, 20.0),
            C(4.9, 20.0, 4.0, 19.1, 4.0, 18.0),
            L(4.0, 6.0),
            C(4.0, 4.9, 4.9, 4.0, 6.0, 4.0),
            Z,
            M(8.0, 20.0),
            L(8.0, 14.0),
            L(16.0, 14.0),
            L(16.0, 20.0),
            M(8.0, 4.0),
            L(8.0, 8.0),
            L(14.0, 8.0),
        ],
        Icon::Edit => vec![
            M(4.0, 20.0),
            L(8.0, 19.0),
            L(19.2, 7.8),
            C(20.2, 6.8, 20.2, 5.4, 19.2, 4.8),
            C(18.6, 3.8, 17.2, 3.8, 16.2, 4.8),
            L(5.0, 16.0),
            Z,
            M(14.5, 6.5),
            L(17.5, 9.5),
        ],
        Icon::App => vec![
            Round(3.0, 4.5, 18.0, 15.0, 3.0),
            M(3.0, 9.0),
            L(21.0, 9.0),
            M(6.8, 6.75),
            L(6.9, 6.75),
        ],
        Icon::Power => vec![
            M(12.0, 3.0),
            L(12.0, 11.0),
            M(7.5, 5.8),
            C(5.0, 7.5, 3.8, 10.3, 4.3, 13.3),
            C(4.9, 17.0, 8.2, 20.0, 12.0, 20.0),
            C(15.8, 20.0, 19.1, 17.0, 19.7, 13.3),
            C(20.2, 10.3, 19.0, 7.5, 16.5, 5.8),
        ],
        Icon::ArrowUpLeft => vec![
            M(17.0, 17.0),
            L(7.0, 7.0),
            M(7.0, 15.0),
            L(7.0, 7.0),
            L(15.0, 7.0),
        ],
        Icon::ArrowUpRight => vec![
            M(7.0, 17.0),
            L(17.0, 7.0),
            M(9.0, 7.0),
            L(17.0, 7.0),
            L(17.0, 15.0),
        ],
        Icon::ArrowDownLeft => vec![
            M(17.0, 7.0),
            L(7.0, 17.0),
            M(7.0, 9.0),
            L(7.0, 17.0),
            L(15.0, 17.0),
        ],
        Icon::ArrowDownRight => vec![
            M(7.0, 7.0),
            L(17.0, 17.0),
            M(9.0, 17.0),
            L(17.0, 17.0),
            L(17.0, 9.0),
        ],
        Icon::Center => vec![
            Circle(12.0, 12.0, 3.0),
            M(4.0, 9.0),
            L(4.0, 5.5),
            C(4.0, 4.7, 4.7, 4.0, 5.5, 4.0),
            L(9.0, 4.0),
            M(15.0, 4.0),
            L(18.5, 4.0),
            C(19.3, 4.0, 20.0, 4.7, 20.0, 5.5),
            L(20.0, 9.0),
            M(20.0, 15.0),
            L(20.0, 18.5),
            C(20.0, 19.3, 19.3, 20.0, 18.5, 20.0),
            L(15.0, 20.0),
            M(9.0, 20.0),
            L(5.5, 20.0),
            C(4.7, 20.0, 4.0, 19.3, 4.0, 18.5),
            L(4.0, 15.0),
        ],
        Icon::ArrowUp => vec![
            M(12.0, 20.0),
            L(12.0, 5.0),
            M(6.0, 11.0),
            L(12.0, 5.0),
            L(18.0, 11.0),
        ],
        Icon::FlipH => vec![
            M(12.0, 4.0),
            L(12.0, 20.0),
            M(8.0, 7.5),
            L(3.5, 17.0),
            L(8.0, 17.0),
            Z,
            M(16.0, 7.5),
            L(20.5, 17.0),
            L(16.0, 17.0),
            Z,
        ],
        Icon::FlipV => vec![
            M(4.0, 12.0),
            L(20.0, 12.0),
            M(7.5, 8.0),
            L(17.0, 8.0),
            L(12.0, 3.5),
            Z,
            M(7.5, 16.0),
            L(17.0, 16.0),
            L(12.0, 20.5),
            Z,
        ],
        Icon::Reset => vec![
            M(4.5, 12.0),
            C(4.5, 7.9, 7.9, 4.5, 12.0, 4.5),
            C(16.1, 4.5, 19.5, 7.9, 19.5, 12.0),
            C(19.5, 16.1, 16.1, 19.5, 12.0, 19.5),
            C(9.4, 19.5, 7.2, 18.2, 5.8, 16.3),
            M(5.0, 20.2),
            L(5.8, 16.3),
            L(9.7, 17.0),
        ],
        Icon::Activity => vec![
            M(3.0, 12.0),
            L(7.0, 12.0),
            L(10.0, 4.5),
            L(14.0, 19.5),
            L(17.0, 12.0),
            L(21.0, 12.0),
        ],
        Icon::Alert => vec![Circle(12.0, 12.0, 9.0), M(12.0, 7.5), L(12.0, 12.5)],
        Icon::Info => vec![Circle(12.0, 12.0, 9.0), M(12.0, 11.0), L(12.0, 16.5)],
    };
    let fills: Vec<S> = match icon {
        Icon::Play => vec![M(8.0, 5.5), L(18.5, 12.0), L(8.0, 18.5), Z],
        Icon::Pause => vec![
            Round(6.5, 5.0, 4.0, 14.0, 1.2),
            Round(13.5, 5.0, 4.0, 14.0, 1.2),
        ],
        Icon::Stop => vec![Round(6.0, 6.0, 12.0, 12.0, 2.2)],
        Icon::Alert => vec![Circle(12.0, 16.2, 1.1)],
        Icon::Info => vec![Circle(12.0, 7.8, 1.1)],
        Icon::FlipH => vec![M(16.0, 7.5), L(20.5, 17.0), L(16.0, 17.0), Z],
        Icon::FlipV => vec![M(7.5, 16.0), L(17.0, 16.0), L(12.0, 20.5), Z],
        _ => Vec::new(),
    };
    (strokes, fills)
}
/// Draw `icon` centred at device coordinates (`cx`, `cy`), `size` device pixels square.
pub fn draw(
    pix: &mut Pixmap,
    icon: Icon,
    cx: f32,
    cy: f32,
    size: f32,
    color: Color,
    mask: Option<&Mask>,
) {
    let (strokes, fills) = geometry(icon);
    let unit = size / 24.0;
    let transform =
        Transform::from_translate(cx - size / 2.0, cy - size / 2.0).pre_scale(unit, unit);
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    if let Some(shape) = path(&fills) {
        pix.fill_path(
            &shape,
            &paint,
            tiny_skia::FillRule::Winding,
            transform,
            mask,
        );
    }
    if let Some(shape) = path(&strokes) {
        // Filled glyphs keep a slim outline for soft, rounded corners.
        let width = if fills.is_empty() { 2.0 } else { 1.4 };
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        pix.stroke_path(&shape, &paint, &stroke, transform, mask);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_icon_paints_pixels_inside_its_square_only() {
        use Icon::*;
        for icon in [
            Search,
            Close,
            Refresh,
            ChevronRight,
            ChevronDown,
            Play,
            Pause,
            Stop,
            Move,
            Check,
            Eye,
            EyeOff,
            Trash,
            Copy,
            Settings,
            Photo,
            Sliders,
            Layers,
            Folder,
            FolderOpen,
            Save,
            Edit,
            App,
            Power,
            ArrowUpLeft,
            ArrowUpRight,
            ArrowDownLeft,
            ArrowDownRight,
            Center,
            ArrowUp,
            FlipH,
            FlipV,
            Reset,
            Activity,
            Alert,
            Info,
        ] {
            let mut pix = Pixmap::new(48, 48).unwrap();
            draw(&mut pix, icon, 24.0, 24.0, 24.0, Color::WHITE, None);
            let mut painted = 0;
            for (i, px) in pix.pixels().iter().enumerate() {
                if px.alpha() > 0 {
                    painted += 1;
                    let (x, y) = (i % 48, i / 48);
                    assert!(
                        (10..38).contains(&x) && (10..38).contains(&y),
                        "{icon:?} leaks outside its square at {x},{y}"
                    );
                }
            }
            assert!(painted > 20, "{icon:?} is blank");
        }
    }
}
