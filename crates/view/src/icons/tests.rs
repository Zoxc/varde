use iced::{Color, Size};

use super::{Icon, Layer, Layers, icon, tinted};
use crate::testing::Laid;
use crate::theme::{IconCategory, IconTone};
use crate::toolbar::tool_icon;
use crate::{ConstraintKind, Message, Mode, Tool};

/// The icons drawn from the icon mock, with accents.
const FROM_MOCK: [Icon; 36] = [
    Icon::Sketch,
    Icon::Extrude,
    Icon::Plane,
    Icon::Line,
    Icon::Circle,
    Icon::Arc,
    Icon::Rectangle,
    Icon::Polygon,
    Icon::Spline,
    Icon::Point,
    Icon::Constrain,
    Icon::Coincident,
    Icon::Horizontal,
    Icon::Vertical,
    Icon::Parallel,
    Icon::Perpendicular,
    Icon::Tangent,
    Icon::Smooth,
    Icon::Equal,
    Icon::Concentric,
    Icon::Midpoint,
    Icon::Symmetric,
    Icon::Fix,
    Icon::Dimension,
    Icon::Trim,
    Icon::Extend,
    Icon::Mirror,
    Icon::Offset,
    Icon::Fillet,
    Icon::Chamfer,
    Icon::CatCreate,
    Icon::CatDraw,
    Icon::CatModify,
    Icon::CatSketchModify,
    Icon::CatConstrain,
    Icon::CatDimension,
];

#[test]
fn every_icon_sorts_into_layers() {
    for icon in Icon::ALL {
        let layers = Layers::of(icon.paths());
        // Nothing is lost: each element lands in one layer.
        let elements = icon.paths().matches("/>").count();
        let sorted = [
            &layers.line,
            &layers.accent,
            &layers.reference,
            &layers.dots,
        ]
        .map(|layer| layer.matches("/>").count());
        assert_eq!(sorted.iter().sum::<usize>(), elements, "{icon:?}");
        assert_eq!(
            layers.holes.matches("/>").count(),
            sorted[3],
            "{icon:?}: a hole per dot"
        );
        assert!(!layers.line.contains("class="), "{icon:?}");
    }
    for icon in FROM_MOCK {
        assert!(icon.category().is_some(), "{icon:?}");
        assert!(icon.draws(Layer::Accent), "{icon:?} has no accent");
    }
    // A dot hides what's under it, drawn outside the mask itself.
    let point = Layers::of(Icon::Coincident.paths()).svg(Layer::All, 0.0);
    assert!(point.contains(r#"<g mask="url(#holes)"><path d="M4 20L20 4M4 4l16 16"/></g>"#));
    assert!(point.ends_with(r#"<circle cx="12" cy="12" r="2.6" stroke-width="1.5"/></svg>"#));
}

#[test]
fn every_toolbar_tool_has_an_icon() {
    let icons = Tool::ALL.map(tool_icon);
    for (tool, icon) in Tool::ALL.iter().zip(icons) {
        assert!(FROM_MOCK.contains(&icon), "{tool:?}: {icon:?}");
        let category = icon.category().unwrap();
        let expected = match tool {
            Tool::Trim | Tool::Extend | Tool::Mirror | Tool::Offset => IconCategory::Modify,
            Tool::Dimension => IconCategory::Dimension,
            _ => IconCategory::Sketch,
        };
        assert_eq!(category, expected, "{tool:?}");
    }
    for (i, icon) in icons.iter().enumerate() {
        assert!(!icons[..i].contains(icon), "{icon:?} twice");
    }
    for kind in ConstraintKind::ALL {
        assert_eq!(kind.icon().category(), Some(IconCategory::Constraint));
        assert!(FROM_MOCK.contains(&kind.icon()), "{kind:?}");
    }
    assert_eq!(
        ConstraintKind::Offset.icon().category(),
        Some(IconCategory::Constraint)
    );
}

/// Whether `pixels` has one within a rounding error of `color`.
fn shows(pixels: &[u8], color: Color) -> bool {
    let want = color.into_rgba8();
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| (0..3).all(|i| pixel[i].abs_diff(want[i]) <= 2))
}

/// Every icon parses and draws: in one colour, and a tool's in its
/// category's three, in both modes.
#[test]
fn icons_draw_in_their_colours() {
    const SIDE: u32 = 48;
    let size = Size::new(SIDE, SIDE);
    let max = Size::new(SIDE as f32, SIDE as f32);
    for mode in [Mode::Light, Mode::Dark] {
        let palette = mode.palette();
        for each in Icon::ALL {
            let pink = Color::from_rgb8(0xff, 0x00, 0xff);
            let pixels =
                Laid::new(tinted(each, SIDE as f32, move |_| pink), max).pixels_in(size, mode);
            assert_eq!(shows(&pixels, pink), each != Icon::Blank, "{each:?}");
        }
        for each in FROM_MOCK.into_iter().chain([Icon::Body, Icon::Convert]) {
            let IconTone {
                line,
                accent,
                reference,
            } = each.tone(palette);
            let pixels = Laid::new(icon::<Message>(each, SIDE as f32), max).pixels_in(size, mode);
            for (layer, color) in [
                (Layer::Line, line),
                (Layer::Accent, accent),
                (Layer::Reference, reference),
            ] {
                assert_eq!(
                    shows(&pixels, color),
                    each.draws(layer),
                    "{each:?} {layer:?} in {mode:?}"
                );
            }
        }
    }
}
