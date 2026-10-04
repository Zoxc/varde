//! Patterns whose copies are bodies of their own: the bodies added, kept
//! by copy across edits, removed with the pattern, and refused when a
//! file lists them wrong.

use super::*;
use crate::{Combine, CombineError, Copies, MAX_PATTERN_BODIES};

/// `pattern` with its copies as bodies of their own, none listed yet.
fn separate(mut pattern: Pattern) -> Pattern {
    pattern.copies = Copies::Separate(Vec::new());
    pattern
}

/// The pattern feature `id` of `editor`'s document.
fn pattern_of(editor: &Editor, id: FeatureId) -> Pattern {
    match &editor.document().feature(id).unwrap().kind {
        FeatureKind::Pattern(pattern) => pattern.clone(),
        other => panic!("not a pattern: {other:?}"),
    }
}

/// The copy bodies of `pattern`.
fn listed(pattern: &Pattern) -> Vec<BodyId> {
    match &pattern.copies {
        Copies::Separate(made) => made.clone(),
        Copies::Joined => Vec::new(),
    }
}

#[test]
fn unjoined_copies_are_new_bodies_laid_out_by_copy() {
    let (mut editor, [a, b], _) = two_bodies();
    let before = editor.document().clone();
    let pattern = separate(linear(&before, &[a, b], "3", "25"));
    let id = add(&mut editor, pattern).unwrap();
    let document = editor.document();
    let pattern = pattern_of(&editor, id);
    let made = listed(&pattern);
    assert_eq!(made.len(), 4, "two bodies, two copies each");
    assert_eq!(pattern.separate_count(), Some(4));
    // Copy k of body i at (k − 1)·n + i, named on from the bodies so far.
    let names: Vec<&str> = (made.iter())
        .map(|&body| document.body(body).unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["Body 3", "Body 4", "Body 5", "Body 6"]);
    for &body in &made {
        let body = document.body(body).unwrap();
        assert_eq!(body.created_by, id);
        assert!(body.visible);
    }
    assert_eq!(pattern.copy_body(0, 1), Some(made[0]));
    assert_eq!(pattern.copy_body(1, 1), Some(made[1]));
    assert_eq!(pattern.copy_body(0, 2), Some(made[2]));
    assert_eq!(pattern.copy_body(1, 2), Some(made[3]));
    assert_eq!(pattern.copy_body(2, 1), None);
    assert_eq!(pattern.copy_body(0, 0), None);
    assert_eq!(pattern.copy_body(0, 3), None);
    let copies: Vec<(BodyId, u32, BodyId)> = pattern.copy_bodies().collect();
    assert_eq!(
        copies,
        [
            (a, 1, made[0]),
            (b, 1, made[1]),
            (a, 2, made[2]),
            (b, 2, made[3])
        ]
    );
    // The originals are left alone; the feature still makes no body of
    // the extrude kind.
    assert_eq!(document.bodies[..2], before.bodies[..]);
    assert_eq!(document.feature(id).unwrap().kind.new_body(), None);
    assert_eq!(document.check(), Ok(()));
    // Through the workers' bytes and back.
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );
    editor.undo();
    assert_eq!(*editor.document(), before);
}

/// Each copy keeps its body across edits: a higher count adds bodies at
/// the end, a lower one removes the last; a body taken out loses its
/// copies; joining removes them all, and unjoining again makes new ones.
#[test]
fn copies_keep_their_bodies_across_edits() {
    let (mut editor, [a, b], _) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, separate(linear(&document, &[a], "3", "25"))).unwrap();
    let first = listed(&pattern_of(&editor, id));
    let set = |editor: &mut Editor, pattern: Pattern| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(pattern.into()),
        })
    };
    // The same pattern set again changes nothing, whatever its list held.
    let revision = editor.revision();
    set(&mut editor, separate(linear(&document, &[a], "3", "25"))).unwrap();
    assert_eq!(editor.revision(), revision);
    // A wider spacing moves the copies; their bodies stay.
    set(&mut editor, separate(linear(&document, &[a], "3", "40"))).unwrap();
    assert_eq!(listed(&pattern_of(&editor, id)), first);
    // Five copies: the two kept, three new after them.
    set(&mut editor, separate(linear(&document, &[a], "5", "40"))).unwrap();
    let five = listed(&pattern_of(&editor, id));
    assert_eq!(five[..2], first[..]);
    assert_eq!(five.len(), 4);
    assert!(five[2] > first[1] && five[2] < five[3]);
    // Back to two: only copy 1 is left.
    set(&mut editor, separate(linear(&document, &[a], "2", "40"))).unwrap();
    assert_eq!(listed(&pattern_of(&editor, id)), first[..1]);
    for &gone in &five[1..] {
        assert!(editor.document().body(gone).is_none());
    }
    // Another body added: a's copy keeps its body, b's is new; the
    // layout is by copy, then by body.
    set(&mut editor, separate(linear(&document, &[a, b], "2", "40"))).unwrap();
    let both = listed(&pattern_of(&editor, id));
    assert_eq!(both[0], first[0]);
    assert_eq!(editor.document().body(both[1]).unwrap().created_by, id);
    // b alone: a's copy goes, b's stays.
    set(&mut editor, separate(linear(&document, &[b], "2", "40"))).unwrap();
    assert_eq!(listed(&pattern_of(&editor, id)), both[1..]);
    // Joined: no copy bodies left.
    let bodies = editor.document().bodies.len();
    set(&mut editor, linear(&document, &[b], "2", "40")).unwrap();
    assert!(pattern_of(&editor, id).joins());
    assert_eq!(editor.document().bodies.len(), bodies - 1);
    // Unjoined again: a new body, not the one removed.
    set(&mut editor, separate(linear(&document, &[b], "2", "40"))).unwrap();
    let again = listed(&pattern_of(&editor, id));
    assert_eq!(again.len(), 1);
    assert!(again[0] > both[1]);
    assert_eq!(editor.document().check(), Ok(()));
    // Undo takes each edit back, the bodies with it.
    editor.undo();
    assert!(pattern_of(&editor, id).joins());
    editor.undo();
    assert_eq!(listed(&pattern_of(&editor, id)), both[1..]);
    editor.redo();
    editor.redo();
    assert_eq!(listed(&pattern_of(&editor, id)), again);
}

/// A copy body a later feature names holds the pattern back from
/// dropping it; removing the pattern removes its bodies and what names
/// them; removing a copy body removes the pattern.
#[test]
fn copy_bodies_are_named_by_later_features_and_go_with_the_pattern() {
    let (mut editor, [a, b], _) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, separate(linear(&document, &[a], "3", "25"))).unwrap();
    let made = listed(&pattern_of(&editor, id));
    let combine = Combine {
        target: b,
        tools: vec![made[1]],
        op: crate::BodyOp::Union,
        keep_tools: false,
    };
    let combined = add(&mut editor, combine).unwrap();
    let held = editor.document().clone();
    // Fewer copies, or joined, would drop the body the combine names.
    for pattern in [
        separate(linear(&document, &[a], "2", "25")),
        linear(&document, &[a], "3", "25"),
    ] {
        let kind = FeatureKind::Pattern(pattern);
        assert!(held.copies_dropped(id, &kind).contains(&made[1]));
        let error = editor
            .apply(Command::SetFeature {
                feature: id,
                kind: Box::new(kind),
            })
            .unwrap_err();
        assert_eq!(
            error,
            EditError::Invalid(CheckError::Combine(combined, CombineError::Body(made[1])))
        );
        assert_eq!(*editor.document(), held);
    }
    // A higher count drops nothing.
    let more = FeatureKind::Pattern(separate(linear(&document, &[a], "4", "25")));
    assert_eq!(held.copies_dropped(id, &more), Vec::<BodyId>::new());
    // Removing the pattern takes its bodies and the combine.
    let removal = held.removal(Removable::Feature(id));
    assert_eq!(removal.features, vec![id, combined]);
    assert_eq!(removal.bodies, made);
    // Removing a copy body removes its maker, the pattern, and so the same.
    assert_eq!(held.removal(Removable::Body(made[0])), removal);
    editor.apply(Command::RemoveBody(made[0])).unwrap();
    assert!(editor.document().feature(id).is_none());
    assert!(
        made.iter()
            .all(|&body| editor.document().body(body).is_none())
    );
    assert_eq!(editor.document().check(), Ok(()));
}

#[test]
fn too_many_copy_bodies_are_refused() {
    let (mut editor, [a, b], _) = two_bodies();
    let document = editor.document().clone();
    let design = document.design();
    // Two bodies, 512 copies each less the originals: 1022 bodies.
    let most = separate(linear(&document, &[a, b], "512", "1"));
    assert_eq!(most.check_own(&design), Ok(()));
    let over = separate(linear(&document, &[a, b], "514", "1"));
    assert_eq!(over.check_own(&design), Err(MotionError::Separate(1026)));
    let next = FeatureId(document.next_id);
    assert_eq!(
        add(&mut editor, over).unwrap_err(),
        EditError::Invalid(CheckError::Pattern(next, MotionError::Separate(1026)))
    );
    // Joined, the count is the only bound.
    assert_eq!(
        linear(&document, &[a, b], "1024", "1").check_own(&design),
        Ok(())
    );
    const { assert!(MAX_PATTERN_BODIES >= 1022) };
    let id = add(&mut editor, most).unwrap();
    assert_eq!(listed(&pattern_of(&editor, id)).len(), 1022);
    assert_eq!(editor.document().check(), Ok(()));
}

/// A file can list copy bodies any way: each wrong one is refused.
#[test]
fn wrong_copy_bodies_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, separate(linear(&document, &[a, b], "2", "25"))).unwrap();
    let document = editor.document().clone();
    let made = listed(&pattern_of(&editor, id));
    let read = |change: &dyn Fn(&mut Document)| {
        let mut document = document.clone();
        change(&mut document);
        Document::from_postcard(&document.to_postcard())
    };
    let copies = |change: &'static dyn Fn(&mut Vec<BodyId>, &[BodyId])| {
        let made = made.clone();
        move |document: &mut Document| {
            let index = document.feature_index(id).unwrap();
            if let FeatureKind::Pattern(Pattern {
                copies: Copies::Separate(list),
                ..
            }) = &mut document.features[index].kind
            {
                change(list, &made);
            }
        }
    };
    let wrong = Err(crate::DecodeError::from(CheckError::Pattern(
        id,
        MotionError::CopyBodies,
    )));
    assert_eq!(read(&|_| {}), Ok(document.clone()));
    // One short, one too many, repeated, swapped for an original.
    assert_eq!(
        read(&copies(&|list, _| {
            list.pop();
        })),
        wrong
    );
    assert_eq!(read(&copies(&|list, made| list.push(made[0]))), wrong);
    assert_eq!(read(&copies(&|list, made| list[1] = made[0])), wrong);
    assert_eq!(read(&copies(&|list, _| list[0] = BodyId(0))), wrong);
    // Joined while it makes bodies.
    let joined = |document: &mut Document| {
        let index = document.feature_index(id).unwrap();
        if let FeatureKind::Pattern(pattern) = &mut document.features[index].kind {
            pattern.copies = Copies::Joined;
        }
    };
    assert_eq!(read(&joined), wrong);
    // A body claiming the pattern made it that it doesn't list.
    let stray = |document: &mut Document| {
        let mut body = document.bodies.last().unwrap().clone();
        body.id = BodyId(document.next_id);
        document.next_id += 1;
        document.bodies.push(body);
    };
    assert_eq!(read(&stray), wrong);
    // A copy body claiming another maker.
    let other = |document: &mut Document| {
        let at = document.body_index(made[0]).unwrap();
        document.bodies[at].created_by = first;
    };
    assert_eq!(
        read(&other),
        Err(crate::DecodeError::from(CheckError::Creator(
            made[0], first
        )))
    );
    // Past the limit.
    let many = |document: &mut Document| {
        let index = document.feature_index(id).unwrap();
        if let FeatureKind::Pattern(pattern) = &mut document.features[index].kind
            && let PatternKind::Linear { count, .. } = &mut pattern.kind
        {
            *count = Value {
                text: "1000".into(),
                value: 1000.0,
            };
        }
    };
    assert_eq!(
        read(&many),
        Err(crate::DecodeError::from(CheckError::Pattern(
            id,
            MotionError::Separate(1998)
        )))
    );
}

/// Copy bodies renamed (as a file can), hidden and made see-through keep
/// all that across edits keeping their copies: another spacing, a higher
/// count, the circular kind for the linear, units; new copies are named
/// on from the highest "Body N", the renamed among them; one dropped and
/// made again is a new body, shown.
#[test]
fn renamed_and_hidden_copy_bodies_keep_it_across_edits() {
    let (mut editor, [a, _], _) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, separate(linear(&document, &[a], "3", "25"))).unwrap();
    let made = listed(&pattern_of(&editor, id));
    let mut renamed = editor.document().clone();
    let at = renamed.body_index(made[0]).unwrap();
    renamed.bodies[at].name = "Body 41".into();
    let at = renamed.body_index(made[1]).unwrap();
    renamed.bodies[at].name = "Spare".into();
    editor.apply(Command::Replace(Box::new(renamed))).unwrap();
    editor.apply(Command::SetVisible(made[0], false)).unwrap();
    let half = crate::Opacity::new(50).unwrap();
    editor.apply(Command::SetOpacity(made[1], half)).unwrap();
    let kept = |editor: &Editor| {
        let document = editor.document();
        let first = document.body(made[0]).unwrap();
        let second = document.body(made[1]).unwrap();
        assert_eq!((first.name.as_str(), first.visible), ("Body 41", false));
        assert_eq!((second.name.as_str(), second.opacity), ("Spare", half));
        assert_eq!(document.check(), Ok(()));
    };
    let set = |editor: &mut Editor, pattern: Pattern| {
        editor
            .apply(Command::SetFeature {
                feature: id,
                kind: Box::new(pattern.into()),
            })
            .unwrap();
    };
    set(&mut editor, separate(linear(&document, &[a], "3", "40")));
    kept(&editor);
    set(&mut editor, separate(linear(&document, &[a], "5", "40")));
    kept(&editor);
    let five = listed(&pattern_of(&editor, id));
    let names: Vec<&str> = (five[2..].iter())
        .map(|&body| editor.document().body(body).unwrap().name.as_str())
        .collect();
    assert_eq!(names, ["Body 42", "Body 43"]);
    set(&mut editor, separate(circular(&document, &[a], "5", "360")));
    kept(&editor);
    assert_eq!(listed(&pattern_of(&editor, id)), five);
    editor
        .apply(Command::SetUnits(varde_expr::LengthUnit::In))
        .unwrap();
    kept(&editor);
    // Two copies: the hidden one stays, the see-through one goes; three
    // again, a new body in its place, shown and opaque.
    set(&mut editor, separate(circular(&document, &[a], "2", "360")));
    assert!(editor.document().body(made[1]).is_none());
    set(&mut editor, separate(circular(&document, &[a], "3", "360")));
    let again = listed(&pattern_of(&editor, id));
    assert_eq!(again[0], made[0]);
    assert!(again[1] > five[3]);
    let new = editor.document().body(again[1]).unwrap();
    assert!(new.visible && new.opacity == crate::Opacity::default());
    assert_eq!(new.name, "Body 42");
    assert!(!editor.document().body(made[0]).unwrap().visible);
}
