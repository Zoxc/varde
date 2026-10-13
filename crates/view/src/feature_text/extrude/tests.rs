use varde_document::{Document, Extent, FeatureKind};
use varde_expr::LengthUnit;

use super::*;

#[test]
fn an_extrude_is_noted_by_its_distances_in_the_units() {
    let length = |value: f64| varde_expr::Value {
        text: String::new(),
        value,
    };
    let mm = LengthUnit::Mm;
    assert_eq!(extent_note(&Extent::OneSide(length(10.0)), mm), "10 mm");
    assert_eq!(
        extent_note(&Extent::OneSide(length(12.7)), LengthUnit::In),
        "0.5 in"
    );
    assert_eq!(
        extent_note(&Extent::Symmetric(length(4.0)), mm),
        "4 mm symmetric"
    );
    assert_eq!(
        extent_note(&Extent::TwoSides(length(10.0), length(2.5)), mm),
        "10 mm + 2.5 mm"
    );
    assert_eq!(extent_note(&Extent::ThroughAll, mm), "Through all");
}

#[test]
fn a_tapered_extrude_is_noted_with_its_taper() {
    let document = Document::example();
    let FeatureKind::Extrude(extrude) = &document.features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let mm = LengthUnit::Mm;
    assert_eq!(extrude_note(extrude, mm), "10 mm");
    let tapered = varde_document::Extrude {
        taper: Some(varde_expr::Value {
            text: "-2".to_owned(),
            value: -(2f64.to_radians()),
        }),
        ..extrude.clone()
    };
    assert_eq!(extrude_note(&tapered, mm), "10 mm · -2°");
}
