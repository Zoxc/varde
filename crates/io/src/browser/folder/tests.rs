//! Browser storage's designs, natively over `FsDir`, whose OS locks stand
//! in for the sync access handles: two `Design`s of one design in this
//! process are two tabs. [`Hooked`] has another tab act in between the
//! steps of one, as the browser may.

use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use pollster::block_on;
use varde_document::Document;

use super::*;
use crate::dir::fs::FsDir;
use crate::tests::{TempDir, with_sketch_named};
use crate::{UnixSeconds, thumbnail};

fn dir(name: &str) -> (TempDir, FsDir) {
    let temp = TempDir::new(name);
    let dir = FsDir(temp.0.clone());
    (temp, dir)
}

fn named(name: &str) -> Document {
    with_sketch_named(name)
}

/// The design `name` saved as a new file in `dir`, held.
fn saved(dir: &FsDir, name: &str, document: &Document) -> Design<File> {
    block_on(save_as(dir, name, false, document, &[])).unwrap()
}

/// The bytes of the file `name` in `dir`.
fn bytes(dir: &FsDir, name: &str) -> Vec<u8> {
    std::fs::read(dir.0.join(name)).unwrap()
}

/// What the design `name` in `dir` holds as its newest save.
fn newest(dir: &FsDir, name: &str) -> Document {
    vrdp::from_bytes(&bytes(dir, name)).unwrap().0
}

fn names(dir: &FsDir) -> Vec<String> {
    let mut names = block_on(dir.names()).unwrap();
    names.sort();
    names
}

/// The designs in `dir` as listed without a store, see [`list`].
fn listing(dir: &FsDir, downloads: &Downloads, held: impl Fn(&str) -> bool) -> Vec<BrowserDesign> {
    let entry = || unreachable!("no store to name an entry in");
    block_on(list(dir, None::<&FsDir>, entry, downloads, held)).designs
}

/// Auto-saves `document` to `design`'s sidecar, based on its file.
fn auto_save(design: &mut Design<File>, document: &Document) {
    let base = Some(design.tail());
    (design.lock().held().unwrap())
        .append(base, None, &Arc::new(document.clone()))
        .unwrap();
}

/// A step of a [`Dir`], as [`Hooked`] tells its hook.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Names,
    Take(String, Make),
    Read(String),
    Remove(String),
    Rename(String, String),
    Pause,
}

/// What [`Hooked`] tells each step.
type Hook<'a> = Box<dyn FnMut(&Step) + 'a>;

/// A [`Dir`] that tells `hook` each step before taking it, so a test can
/// have another tab act in between, through the [`FsDir`] underneath.
struct Hooked<'a> {
    dir: FsDir,
    hook: RefCell<Hook<'a>>,
}

impl<'a> Hooked<'a> {
    fn new(dir: &FsDir, hook: impl FnMut(&Step) + 'a) -> Self {
        Self {
            dir: dir.clone(),
            hook: RefCell::new(Box::new(hook)),
        }
    }

    fn step(&self, step: Step) {
        (self.hook.borrow_mut())(&step);
    }
}

impl Dir for Hooked<'_> {
    type File = File;
    type Reader = File;

    async fn names(&self) -> io::Result<Vec<String>> {
        self.step(Step::Names);
        self.dir.names().await
    }

    async fn take(&self, name: &str, make: Make) -> io::Result<File> {
        self.step(Step::Take(name.to_owned(), make));
        self.dir.take(name, make).await
    }

    async fn read(&self, name: &str, max: usize) -> io::Result<Vec<u8>> {
        self.step(Step::Read(name.to_owned()));
        self.dir.read(name, max).await
    }

    async fn reader(&self, name: &str) -> io::Result<File> {
        self.step(Step::Read(name.to_owned()));
        self.dir.reader(name).await
    }

    async fn pause(&self, millis: u32) {
        self.step(Step::Pause);
        self.dir.pause(millis).await;
    }

    async fn modified(&self, name: &str) -> Option<UnixSeconds> {
        self.dir.modified(name).await
    }

    async fn remove(&self, name: &str) -> io::Result<()> {
        self.step(Step::Remove(name.to_owned()));
        self.dir.remove(name).await
    }

    async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.step(Step::Rename(from.to_owned(), to.to_owned()));
        self.dir.rename(from, to).await
    }
}

/// Saved as a name, then saved again: each save appended, so what was
/// saved before is still there, the history kept, as natively.
#[test]
fn saves_keep_the_history() {
    let (_temp, dir) = dir("saved-history");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    assert_eq!(design.name(), "a.vrdp");
    assert_eq!(design.access(), Access::Edit);
    assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp"]);
    let first = bytes(&dir, "a.vrdp");
    block_on(save(&dir, &mut design, &named("two"), &[])).unwrap();
    let second = bytes(&dir, "a.vrdp");
    assert!(second.len() > first.len() && second.starts_with(&first));
    assert_eq!(newest(&dir, "a.vrdp"), named("two"));
    // Another save from what's known of it after the last.
    block_on(save(&dir, &mut design, &named("three"), &[])).unwrap();
    assert!(bytes(&dir, "a.vrdp").starts_with(&second));
    block_on(close(&dir, design, Ending::Close)).unwrap();
    assert_eq!(names(&dir), ["a.vrdp"]);
}

/// A save over a file someone else changed since is refused, as natively.
#[test]
fn a_save_over_someone_else_s_is_a_conflict() {
    let (_temp, dir) = dir("saved-conflict");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    std::fs::write(
        dir.0.join("a.vrdp"),
        vrdp::to_bytes(&named("theirs"), &[]).unwrap().0,
    )
    .unwrap();
    assert_eq!(
        block_on(save(&dir, &mut design, &named("two"), &[])),
        Err(SaveError::Conflict)
    );
    assert_eq!(newest(&dir, "a.vrdp"), named("theirs"));
}

/// The sidecar's handle is the lock: a second tab opens the design
/// read-only, can't save it, save over it, auto-save it, rename it,
/// delete it or save another design as its name, till the first lets go.
#[test]
fn one_tab_edits_a_design_at_a_time() {
    let (_temp, dir) = dir("saved-tabs");
    let first = saved(&dir, "a.vrdp", &named("one"));
    let mut second = block_on(open(&dir, "a.vrdp")).unwrap();
    assert_eq!(second.design.access(), Access::ReadOnly(ReadOnly::InUse));
    assert!(second.design.lock().held().is_none());
    assert_eq!(second.read.opened.payload, named("one"));
    assert!(matches!(second.recovered, Ok(None)));
    let read_only = Err(SaveError::Failed(READ_ONLY.to_owned()));
    assert_eq!(
        block_on(save(&dir, &mut second.design, &named("two"), &[])),
        read_only
    );
    assert_eq!(
        block_on(save_over(&dir, &mut second.design, &named("two"), &[])),
        read_only
    );
    assert!(block_on(rename(&dir, &mut second.design, "b.vrdp")).is_err());
    assert!(matches!(
        block_on(save_as(&dir, "a.vrdp", true, &named("two"), &[])),
        Err(SaveError::Failed(e)) if e.contains("open elsewhere")
    ));
    assert!(
        block_on(delete(&dir, "a.vrdp", false))
            .unwrap_err()
            .contains("open")
    );
    // Nor by this tab, which holds it.
    assert!(block_on(delete(&dir, "a.vrdp", true)).is_err());
    // Closing the read-only one lets go of nothing.
    block_on(close(&dir, second.design, Ending::Close)).unwrap();
    assert_eq!(newest(&dir, "a.vrdp"), named("one"));
    assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp"]);

    block_on(close(&dir, first, Ending::Close)).unwrap();
    let again = block_on(open(&dir, "a.vrdp")).unwrap();
    assert_eq!(again.design.access(), Access::Edit);
}

/// Another tab holding a design's sidecar a moment, as it lists the
/// designs, is waited for: opening, saving as, renaming to and deleting
/// try again, a few times, before taking the design to be open elsewhere.
#[test]
fn a_sidecar_held_a_moment_is_waited_for() {
    let (_temp, dir) = dir("saved-wait");
    let design = saved(&dir, "a.vrdp", &named("one"));
    block_on(close(&dir, design, Ending::Close)).unwrap();
    // Another tab listing holds a design's sidecar till the second pause.
    let listing = RefCell::new(None);
    let pauses = RefCell::new(0);
    let hooked = Hooked::new(&dir, |step| {
        if *step == Step::Pause {
            *pauses.borrow_mut() += 1;
            if *pauses.borrow() == 2 {
                listing.borrow_mut().take();
            }
        }
    });
    let hold = |name: &str| {
        *pauses.borrow_mut() = 0;
        let sidecar = block_on(dir.take(&sidecar_name(name), Make::IfMissing)).unwrap();
        *listing.borrow_mut() = Some(sidecar);
    };
    hold("a.vrdp");
    let opened = block_on(open(&hooked, "a.vrdp")).unwrap();
    assert_eq!(opened.design.access(), Access::Edit);
    assert_eq!(*pauses.borrow(), 2);
    block_on(close(&hooked, opened.design, Ending::Close)).unwrap();
    hold("a.vrdp");
    let mut design = block_on(save_as(&hooked, "a.vrdp", true, &named("two"), &[])).unwrap();
    hold("b.vrdp");
    block_on(rename(&hooked, &mut design, "b.vrdp")).unwrap();
    block_on(close(&hooked, design, Ending::Close)).unwrap();
    hold("b.vrdp");
    block_on(delete(&hooked, "b.vrdp", false)).unwrap();
    assert_eq!(*pauses.borrow(), 2);
    assert!(names(&dir).is_empty());
    // Held for good, by a tab with it open: read-only, after the pauses.
    let _open = saved(&dir, "c.vrdp", &named("three"));
    *pauses.borrow_mut() = 0;
    let other = block_on(open(&hooked, "c.vrdp")).unwrap();
    assert_eq!(other.design.access(), Access::ReadOnly(ReadOnly::InUse));
    assert_eq!(*pauses.borrow(), 4);
}

/// Auto-saves go to the sidecar, never the design. A tab closed with
/// changes leaves them there: listed as changes not saved, and offered
/// when the design is next opened, as natively after a crash; a save
/// leaves an offer not answered be, and discarding it empties it.
#[test]
fn changes_a_closed_tab_left_are_offered() {
    let (_temp, dir) = dir("saved-recovery");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    let file = bytes(&dir, "a.vrdp");
    auto_save(&mut design, &named("edited"));
    assert_eq!(bytes(&dir, "a.vrdp"), file);
    // The tab closed: the handle goes, the sidecar stays.
    drop(design);
    let listed = listing(&dir, &Downloads::default(), |_| false);
    assert_eq!(listed.len(), 1);
    assert!(listed[0].unsaved && !listed[0].in_use, "{listed:?}");

    let mut opened = block_on(open(&dir, "a.vrdp")).unwrap();
    let offer = opened.recovered.unwrap().unwrap();
    assert_eq!(offer.document, named("edited"));
    assert!(!offer.design_changed && !offer.newer_base && offer.damage.is_none());
    assert!(opened.design.lock().offered());
    // Saved meanwhile: the offer stays till answered.
    block_on(save(&dir, &mut opened.design, &named("two"), &[])).unwrap();
    let sidecar = sidecar_name("a.vrdp");
    assert!(!bytes(&dir, &sidecar).is_empty());
    opened.design.lock().held().unwrap().clear().unwrap();
    opened.design.lock().answered();
    block_on(close(&dir, opened.design, Ending::Close)).unwrap();
    assert_eq!(names(&dir), ["a.vrdp"]);

    // Saved by another tab since the changes were made: said so.
    let mut design = block_on(open(&dir, "a.vrdp")).unwrap().design;
    auto_save(&mut design, &named("edited"));
    drop(design);
    let mut other = block_on(open(&dir, "a.vrdp")).unwrap().design;
    other.lock().answered();
    // Kept, as `Release` keeps what's in it.
    block_on(close(&dir, other, Ending::Release)).unwrap();
    let mut writer = block_on(dir.take("a.vrdp", Make::No)).unwrap();
    let mut known = vrdp::from_bytes_with_report(&bytes(&dir, "a.vrdp"))
        .unwrap()
        .known();
    vrdp::save(&mut writer, &mut known, &named("three"), &[]).unwrap();
    drop(writer);
    let opened = block_on(open(&dir, "a.vrdp")).unwrap();
    assert!(opened.recovered.unwrap().unwrap().design_changed);
}

/// The `n`th save of a design, told apart by a sketch's name.
fn version(n: usize) -> Document {
    named(&format!("Save {n}"))
}

/// Flips the byte at `at` in the file `name` in `dir`.
fn flip(dir: &FsDir, name: &str, at: u64) {
    let mut bytes = bytes(dir, name);
    bytes[at as usize] ^= 0xff;
    std::fs::write(dir.0.join(name), bytes).unwrap();
}

/// Where `prev` is in a block.
const PREV_AT: u64 = 8;
/// Somewhere in a block's tag or payload, past its header.
const BODY_AT: u64 = 60;

/// A design damaged past an early save opens that save, offering the one
/// a search found: gone over to, what a closed tab left, based on the save
/// found, is offered again against it, as of the design unchanged.
#[test]
fn the_save_found_past_damage_offers_changes_again() {
    let (_temp, dir) = dir("saved-found");
    let mut design = saved(&dir, "a.vrdp", &version(1));
    let mut tails = vec![design.tail()];
    for n in 2..=4 {
        block_on(save(&dir, &mut design, &version(n), &[])).unwrap();
        tails.push(design.tail());
    }
    auto_save(&mut design, &named("edited"));
    drop(design);
    flip(&dir, "a.vrdp", tails[1].span().start + PREV_AT);

    let mut opened = block_on(open(&dir, "a.vrdp")).unwrap();
    assert_eq!(opened.read.opened.payload, version(1));
    let offer = opened.recovered.unwrap().unwrap();
    // Based on a save of the design other than the one opened.
    assert!(offer.design_changed);
    let found = opened.read.found.take().unwrap();
    assert_eq!(found.tail, tails[3]);
    let document = opened.design.open_found(found);
    assert_eq!(document, version(4));
    assert_eq!(opened.design.tail(), tails[3]);
    let offer = block_on(offer_again(&dir, &mut opened.design, &document))
        .unwrap()
        .unwrap();
    assert_eq!(offer.document, named("edited"));
    assert!(!offer.design_changed && !offer.newer_base);
    assert!(opened.design.lock().offered());
    // Saves are still refused: the damage would be cut off.
    assert_eq!(
        block_on(save(&dir, &mut opened.design, &version(5), &[])),
        Err(SaveError::OpenedDamaged)
    );
}

/// Saving as another name writes a new file of one save under its own
/// lock, refused over a design there unless asked to replace it, as
/// [`SaveError::Taken`]; replaced, what a closed tab left of the design
/// replaced goes.
#[test]
fn saving_as_a_name_in_use_asks_first() {
    let (_temp, dir) = dir("saved-save-as");
    let first = saved(&dir, "a.vrdp", &named("one"));
    block_on(close(&dir, first, Ending::Close)).unwrap();
    assert_eq!(
        block_on(save_as(&dir, "a.vrdp", false, &named("two"), &[])).map(|_| ()),
        Err(SaveError::Taken)
    );
    // The sidecar taken for it is let go of again, the temporary file
    // gone too.
    assert_eq!(names(&dir), ["a.vrdp"]);
    assert_eq!(newest(&dir, "a.vrdp"), named("one"));
    // A closed tab's changes to the design replaced.
    let mut left = block_on(open(&dir, "a.vrdp")).unwrap().design;
    auto_save(&mut left, &named("left"));
    drop(left);
    let mut replaced = block_on(save_as(&dir, "a.vrdp", true, &named("two"), &[])).unwrap();
    assert_eq!(newest(&dir, "a.vrdp"), named("two"));
    assert!(replaced.lock().held().unwrap().read().unwrap().is_none());
    // One save, written whole.
    let whole = vrdp::to_bytes(&named("two"), &[]).unwrap().0;
    assert_eq!(bytes(&dir, "a.vrdp").len(), whole.len());
    assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp"]);
    // Saving over itself writes it whole again, keeping its lock: another
    // tab still opens it read-only.
    block_on(save(&dir, &mut replaced, &named("three"), &[])).unwrap();
    block_on(save_over(&dir, &mut replaced, &named("four"), &[])).unwrap();
    let whole = vrdp::to_bytes(&named("four"), &[]).unwrap().0;
    assert_eq!(bytes(&dir, "a.vrdp").len(), whole.len());
    assert_eq!(newest(&dir, "a.vrdp"), named("four"));
    assert_eq!(replaced.access(), Access::Edit);
    let other = block_on(open(&dir, "a.vrdp")).unwrap();
    assert_eq!(other.design.access(), Access::ReadOnly(ReadOnly::InUse));
    block_on(save(&dir, &mut replaced, &named("five"), &[])).unwrap();
    assert_eq!(newest(&dir, "a.vrdp"), named("five"));
    assert!(matches!(
        block_on(save_as(&dir, "../a.vrdp", true, &named("x"), &[])),
        Err(SaveError::Failed(_))
    ));
}

/// A design is only ever put in place whole: saved as a new name, it's
/// written under a temporary name and moved there, as over another; a
/// file another tab made of the name meanwhile is kept, and the save is
/// [`SaveError::Taken`].
#[test]
fn a_new_design_is_written_whole_before_it_is_in_place() {
    let (_temp, dir) = dir("saved-whole");
    let steps = RefCell::new(Vec::new());
    let hooked = Hooked::new(&dir, |step| steps.borrow_mut().push(step.clone()));
    let design = block_on(save_as(&hooked, "a.vrdp", false, &named("one"), &[])).unwrap();
    let steps = steps.take();
    assert!(!steps.contains(&Step::Take("a.vrdp".to_owned(), Make::New)));
    let moved = (steps.iter()).find_map(|step| match step {
        Step::Rename(from, to) if to == "a.vrdp" => Some(from.clone()),
        _ => None,
    });
    assert_eq!(moved.as_deref().and_then(temp_of), Some("a.vrdp"));
    assert_eq!(newest(&dir, "a.vrdp"), named("one"));
    drop(design);

    // Another tab saves the name just before it's moved there.
    let hooked = Hooked::new(&dir, |step| {
        if let Step::Rename(_, to) = step
            && to == "b.vrdp"
        {
            std::fs::write(dir.0.join("b.vrdp"), b"theirs").unwrap();
        }
    });
    assert_eq!(
        block_on(save_as(&hooked, "b.vrdp", false, &named("two"), &[])).map(|_| ()),
        Err(SaveError::Taken)
    );
    assert_eq!(bytes(&dir, "b.vrdp"), b"theirs");
    assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp", "b.vrdp"]);
}

/// Another tab listing as a design is saved over never touches what's
/// being written: before the design replaced is deleted, or after, its
/// temporary file is the writer's, who holds the design's sidecar, so the
/// design is never lost, nor the save said to fail when it didn't.
#[test]
fn listing_leaves_a_design_being_written_be() {
    let (_temp, dir) = dir("saved-race");
    let design = saved(&dir, "a.vrdp", &named("one"));
    block_on(close(&dir, design, Ending::Close)).unwrap();
    for before in [true, false] {
        // The other tab lists as the writer deletes the design replaced,
        // or as it moves the new file in its place.
        let listed = RefCell::new(false);
        let hooked = Hooked::new(&dir, |step| {
            let now = match step {
                Step::Remove(name) => before && name == "a.vrdp",
                Step::Rename(from, _) => !before && temp_of(from).is_some(),
                _ => false,
            };
            if now && !listed.replace(true) {
                listing(&dir, &Downloads::default(), |_| false);
            }
        });
        let new = named(if before { "two" } else { "three" });
        let design = block_on(save_as(&hooked, "a.vrdp", true, &new, &[])).unwrap();
        assert!(*listed.borrow());
        assert_eq!(newest(&dir, "a.vrdp"), new);
        assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp"]);
        block_on(close(&dir, design, Ending::Close)).unwrap();
    }
    // Saving over itself, the same.
    let mut design = block_on(open(&dir, "a.vrdp")).unwrap().design;
    let listed = RefCell::new(false);
    let hooked = Hooked::new(&dir, |step| {
        if matches!(step, Step::Remove(name) if name == "a.vrdp") && !listed.replace(true) {
            listing(&dir, &Downloads::default(), |_| false);
        }
    });
    block_on(save_over(&hooked, &mut design, &named("four"), &[])).unwrap();
    assert!(*listed.borrow());
    assert_eq!(newest(&dir, "a.vrdp"), named("four"));
    assert_eq!(names(&dir), [".a.vrdp.autosave", "a.vrdp"]);
}

/// Renaming moves the file, saves and all, and the auto-saves with it,
/// refused over a design there.
#[test]
fn renaming_keeps_the_history() {
    let (_temp, dir) = dir("saved-rename");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    block_on(save(&dir, &mut design, &named("two"), &[])).unwrap();
    let before = bytes(&dir, "a.vrdp");
    auto_save(&mut design, &named("edited"));
    let other = saved(&dir, "b.vrdp", &named("other"));
    block_on(close(&dir, other, Ending::Close)).unwrap();
    assert!(
        block_on(rename(&dir, &mut design, "b.vrdp"))
            .unwrap_err()
            .contains("already")
    );
    assert_eq!(design.name(), "a.vrdp");
    block_on(rename(&dir, &mut design, "c.vrdp")).unwrap();
    assert_eq!(design.name(), "c.vrdp");
    assert_eq!(bytes(&dir, "c.vrdp"), before);
    assert_eq!(names(&dir), [".c.vrdp.autosave", "b.vrdp", "c.vrdp"]);
    let moved = design.lock().held().unwrap().read().unwrap().unwrap();
    assert_eq!(*moved.document, named("edited"));
    assert!(moved.based_on(design.tail()));
    // Saves go on from where they were.
    block_on(save(&dir, &mut design, &named("three"), &[])).unwrap();
    assert!(bytes(&dir, "c.vrdp").starts_with(&before));
    block_on(rename(&dir, &mut design, "c.vrdp")).unwrap();
}

/// Renamed while what a closed tab left is offered, the offer moves to
/// the new sidecar with it, still offered: answering it there answers it.
#[test]
fn renaming_keeps_an_offer() {
    let (_temp, dir) = dir("saved-rename-offer");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    auto_save(&mut design, &named("left"));
    drop(design);
    let mut opened = block_on(open(&dir, "a.vrdp")).unwrap();
    assert!(opened.design.lock().offered());
    block_on(rename(&dir, &mut opened.design, "b.vrdp")).unwrap();
    assert!(opened.design.lock().offered());
    assert_eq!(names(&dir), [".b.vrdp.autosave", "b.vrdp"]);
    // A save leaves it be, as it's still offered.
    block_on(save(&dir, &mut opened.design, &named("two"), &[])).unwrap();
    let held = opened.design.lock().held().unwrap();
    assert_eq!(*held.read().unwrap().unwrap().document, named("left"));
    // Closed without answering, it's offered when next opened.
    block_on(close(&dir, opened.design, Ending::Release)).unwrap();
    let opened = block_on(open(&dir, "b.vrdp")).unwrap();
    assert_eq!(opened.recovered.unwrap().unwrap().document, named("left"));
}

/// Saved as another design, one lets go of its sidecar, keeping what a
/// closed tab left in it only while that's still offered.
#[test]
fn saved_as_another_keeps_only_an_offer() {
    let (_temp, dir) = dir("saved-elsewhere");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    assert_eq!(ending_for_saved_as(&design), Ending::Close);
    auto_save(&mut design, &named("edited"));
    drop(design);
    let mut opened = block_on(open(&dir, "a.vrdp")).unwrap();
    assert_eq!(ending_for_saved_as(&opened.design), Ending::Release);
    opened.design.lock().answered();
    assert_eq!(ending_for_saved_as(&opened.design), Ending::Close);
    assert_eq!(super::super::DIR, "saved");
}

/// Deleting takes the design and its sidecar, what it holds too.
#[test]
fn deleting_takes_the_sidecar_too() {
    let (_temp, dir) = dir("saved-delete");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    auto_save(&mut design, &named("edited"));
    drop(design);
    block_on(delete(&dir, "a.vrdp", false)).unwrap();
    assert!(names(&dir).is_empty());
    assert!(block_on(delete(&dir, "b.txt", false)).is_err());
    // One gone already is deleted all the same.
    block_on(delete(&dir, "a.vrdp", false)).unwrap();
    assert!(names(&dir).is_empty());
}

#[test]
fn copies_get_names_of_their_own() {
    let taken = |names: &[&str]| {
        let names: Vec<String> = names.iter().map(|&name| name.to_owned()).collect();
        move |name: &str| names.iter().any(|taken| taken == name)
    };
    assert_eq!(
        copy_name("bracket.vrdp", taken(&[])).unwrap(),
        "bracket.vrdp"
    );
    assert_eq!(
        copy_name("bracket.vrdp", taken(&["bracket.vrdp"])).unwrap(),
        "bracket (2).vrdp"
    );
    assert_eq!(
        copy_name("bracket.vrdp", taken(&["bracket.vrdp", "bracket (2).vrdp"])).unwrap(),
        "bracket (3).vrdp"
    );
    assert_eq!(copy_name("part", taken(&[])).unwrap(), "part.vrdp");
    assert_eq!(copy_name("../x.vrdp", taken(&[])).unwrap(), "_x.vrdp");
    assert_eq!(copy_name("a", |_| true), None);
}

/// A name too long for browser storage is cut to fit, its sidecar's and
/// temporary file's names within a file name's 255 bytes; a copy's number
/// cuts the stem, never itself.
#[test]
fn long_names_are_cut_to_fit() {
    use super::super::{MAX_NAME, file_name};
    let long = format!("{}.vrdp", "é".repeat(200));
    let name = file_name(&long);
    assert!(name.len() <= MAX_NAME && is_design_name(&name), "{name}");
    assert!(name.ends_with("é.vrdp"));
    assert!(temp_name(&name, u64::MAX).len() <= 255);
    assert!(sidecar_name(&name).len() <= 255);
    let copy = copy_name(&long, |taken| taken == name).unwrap();
    assert!(copy.ends_with("é (2).vrdp"), "{copy}");
    assert!(copy.len() <= MAX_NAME && is_design_name(&copy));
    let copy = copy_name(&long, |taken| !taken.contains("(999)")).unwrap();
    assert!(copy.ends_with(" (999).vrdp") && copy.len() <= MAX_NAME);
    // Names past it aren't designs': nothing makes them.
    assert!(!is_design_name(&format!("{}.vrdp", "a".repeat(MAX_NAME))));
    assert!(matches!(
        block_on(save_as(
            &dir("saved-long").1,
            &format!("{}.vrdp", "a".repeat(250)),
            false,
            &named("x"),
            &[]
        )),
        Err(SaveError::Failed(_))
    ));
}

/// A file opened from a file input is copied in as it is, its history
/// with it, under a name of its own, and opens from there.
#[test]
fn files_are_copied_in_under_a_name_of_their_own() {
    let (_temp, dir) = dir("saved-copy");
    let file = vrdp::to_bytes(&named("one"), &[]).unwrap().0;
    let copy = |name: &str| {
        let read = vrdp::from_bytes_with_found(&file).unwrap();
        match block_on(copy_in(&dir, name, &file, read)) {
            Ok(opened) => opened,
            Err(not_copied) => panic!("{}", not_copied.0),
        }
    };
    let first = copy("bracket.vrdp");
    assert_eq!(first.design.name(), "bracket.vrdp");
    let second = copy("bracket.vrdp");
    assert_eq!(second.design.name(), "bracket (2).vrdp");
    assert_eq!(bytes(&dir, "bracket (2).vrdp"), file);
    assert_eq!(second.read.opened.payload, named("one"));
    assert_eq!(second.design.access(), Access::Edit);
    // A name with a file held by another tab is another's.
    let held = block_on(dir.take("x.vrdp", Make::New)).unwrap();
    assert_eq!(copy("x.vrdp").design.name(), "x (2).vrdp");
    drop(held);
    // So is one whose sidecar is there, even with no design: a tab closed
    // as it renamed one left changes in it, which aren't the copy's.
    std::fs::write(dir.0.join(".y.vrdp.autosave"), b"left").unwrap();
    let y = copy("y.vrdp");
    assert_eq!(y.design.name(), "y (2).vrdp");
    assert!(matches!(y.recovered, Ok(None)));
    assert_eq!(bytes(&dir, ".y.vrdp.autosave"), b"left");
}

/// Another tab copying a file of the same name in between listing the
/// names and taking one: the next name, theirs kept.
#[test]
fn a_name_taken_meanwhile_is_passed_over() {
    let (_temp, dir) = dir("saved-copy-race");
    let file = vrdp::to_bytes(&named("one"), &[]).unwrap().0;
    let theirs = vrdp::to_bytes(&named("theirs"), &[]).unwrap().0;
    let hooked = Hooked::new(&dir, |step| {
        // Theirs made as this one takes the sidecar of the name: it isn't
        // there yet as the names are listed, nor its sidecar.
        if *step == Step::Take(".x.vrdp.autosave".to_owned(), Make::New) {
            std::fs::write(dir.0.join("x.vrdp"), &theirs).unwrap();
        }
    });
    let read = vrdp::from_bytes_with_found(&file).unwrap();
    let Ok(opened) = block_on(copy_in(&hooked, "x.vrdp", &file, read)) else {
        panic!("not copied");
    };
    assert_eq!(opened.design.name(), "x (2).vrdp");
    assert_eq!(bytes(&dir, "x.vrdp"), theirs);
    assert_eq!(bytes(&dir, "x (2).vrdp"), file);
    drop(opened);
    assert_eq!(
        names(&dir),
        [".x (2).vrdp.autosave", "x (2).vrdp", "x.vrdp"]
    );
}

/// Opening refuses what isn't a design before taking a lock for it.
#[test]
fn only_designs_open() {
    let (_temp, dir) = dir("saved-open");
    assert!(
        block_on(open(&dir, "missing.vrdp"))
            .unwrap_err()
            .contains("isn't in")
    );
    std::fs::write(dir.0.join("text.vrdp"), "not a design").unwrap();
    assert!(block_on(open(&dir, "text.vrdp")).is_err());
    assert!(block_on(open(&dir, ".x.vrdp.autosave")).is_err());
    assert_eq!(names(&dir), ["text.vrdp"]);
}

/// Sets when the file `name` in `dir` was last written to `seconds` after
/// the epoch.
fn written_at(dir: &FsDir, name: &str, seconds: u64) {
    let file = File::options().write(true).open(dir.0.join(name)).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

/// The listing: newest first, each with its time, thumbnail, where it
/// stands against its downloads, and whether it's open elsewhere or holds
/// changes; what isn't a design marked so. Designs left under a temporary
/// name are put in place, other temporary files and empty sidecars of
/// designs gone deleted.
#[test]
fn designs_are_listed_newest_first() {
    let (_temp, dir) = dir("saved-list");
    let image = thumbnail::Image::new(2, 1, vec![9; 8]).unwrap();
    let previews = thumbnail::previews(Some(&image));
    let old = block_on(save_as(&dir, "old.vrdp", false, &named("old"), &[])).unwrap();
    block_on(close(&dir, old, Ending::Close)).unwrap();
    let new = block_on(save_as(&dir, "new.vrdp", false, &named("new"), &previews)).unwrap();
    let sum = new.tail().sum();
    std::fs::write(dir.0.join("bad.vrdp"), "not a design").unwrap();
    // A design put under a temporary name by a tab closed before putting
    // it in place, another left beside its design, and an empty sidecar
    // of a design gone.
    let whole = vrdp::to_bytes(&named("moved"), &[]).unwrap().0;
    std::fs::write(dir.0.join(".moved.vrdp.1a.tmp"), &whole).unwrap();
    std::fs::write(dir.0.join(".old.vrdp.2b.tmp"), &whole).unwrap();
    std::fs::write(dir.0.join(".gone.vrdp.autosave"), "").unwrap();
    std::fs::write(dir.0.join("notes.txt"), "kept").unwrap();
    for (name, at) in [
        ("old.vrdp", 1000),
        ("new.vrdp", 3000),
        (".moved.vrdp.1a.tmp", 2000),
        ("bad.vrdp", 500),
    ] {
        written_at(&dir, name, at);
    }

    let mut downloads = Downloads::default();
    downloads.record("new.vrdp", UnixSeconds(5), Some(sum));
    downloads.record("old.vrdp", UnixSeconds(4), None);
    let listed = listing(&dir, &downloads, |_| false);
    let shown: Vec<_> = (listed.iter())
        .map(|design| (design.name.as_str(), design.saved, design.download))
        .collect();
    assert_eq!(
        shown,
        [
            (
                "new.vrdp",
                Some(UnixSeconds(3000)),
                DownloadStatus::Latest(UnixSeconds(5))
            ),
            ("moved.vrdp", Some(UnixSeconds(2000)), DownloadStatus::Never),
            (
                "old.vrdp",
                Some(UnixSeconds(1000)),
                DownloadStatus::Changed(UnixSeconds(4))
            ),
            ("bad.vrdp", Some(UnixSeconds(500)), DownloadStatus::Never),
        ]
    );
    let find = |name: &str| listed.iter().find(|design| design.name == name).unwrap();
    assert_eq!(find("new.vrdp").thumbnail, Some(image));
    assert_eq!(find("new.vrdp").sum, Some(sum));
    assert!(find("new.vrdp").in_use && !find("new.vrdp").unsaved);
    assert_eq!(find("bad.vrdp").damage, Some(ListedDamage::Unreadable));
    assert_eq!(find("bad.vrdp").sum, None);
    assert_eq!(
        names(&dir),
        [
            ".new.vrdp.autosave",
            "bad.vrdp",
            "moved.vrdp",
            "new.vrdp",
            "notes.txt",
            "old.vrdp"
        ]
    );
    assert_eq!(bytes(&dir, "moved.vrdp"), whole);
    // One this tab holds isn't looked at past its file.
    let listed = listing(&dir, &downloads, |name| name == "new.vrdp");
    assert!(find_in(&listed, "new.vrdp").in_use);
    drop(new);
    let listed = listing(&dir, &downloads, |_| false);
    assert!(!find_in(&listed, "new.vrdp").in_use);
    // Its empty sidecar went as it was listed.
    assert!(!names(&dir).contains(&".new.vrdp.autosave".to_owned()));
}

/// Listing reads the end of each design file, never the saves before:
/// damage there shows as the design opens, not in the listing.
#[test]
fn listing_reads_only_the_end_of_a_file() {
    let (_temp, dir) = dir("saved-list-end");
    let mut design = saved(&dir, "a.vrdp", &version(1));
    let first = design.tail();
    block_on(save(&dir, &mut design, &version(2), &[])).unwrap();
    let sum = design.tail().sum();
    drop(design);
    flip(&dir, "a.vrdp", first.span().start + BODY_AT);
    let listed = listing(&dir, &Downloads::default(), |_| false);
    assert_eq!((listed[0].sum, listed[0].damage), (Some(sum), None));
    assert!(
        block_on(open(&dir, "a.vrdp"))
            .unwrap()
            .read
            .opened
            .report
            .unreadable
            > 0
    );
}

fn find_in<'a>(listed: &'a [BrowserDesign], name: &str) -> &'a BrowserDesign {
    listed.iter().find(|design| design.name == name).unwrap()
}

/// What a tab closed as it renamed a design left in the old name's
/// sidecar, a sidecar of no design with changes in it, goes to the store
/// of new designs as listing finds it, known by the old name, rather than
/// stay hidden; one that can't be read stays as it is, as do all of them
/// without a store.
#[test]
fn changes_left_of_no_design_go_to_the_store() {
    let (_temp, dir) = dir("saved-orphan");
    let (_store_temp, store) = self::dir("saved-orphan-store");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    auto_save(&mut design, &named("edited"));
    drop(design);
    // The design moved, its sidecar left behind.
    std::fs::rename(dir.0.join("a.vrdp"), dir.0.join("b.vrdp")).unwrap();
    std::fs::write(dir.0.join(".c.vrdp.autosave"), b"not a held file").unwrap();
    // Without a store, nothing moves.
    listing(&dir, &Downloads::default(), |_| false);
    assert!(names(&dir).contains(&".a.vrdp.autosave".to_owned()));

    let mut n = 0;
    let entry = || {
        n += 1;
        format!("entry-{n}.vrdp")
    };
    let listed = block_on(list(
        &dir,
        Some(&store),
        entry,
        &Downloads::default(),
        |_| false,
    ));
    assert!(listed.rescued);
    assert_eq!(listed.designs.len(), 1);
    assert_eq!(names(&dir), [".c.vrdp.autosave", "b.vrdp"]);
    assert_eq!(names(&store), ["entry-1.vrdp"]);
    let mut entry = Held::new(block_on(store.take("entry-1.vrdp", Make::No)).unwrap());
    let rescued = entry.read().unwrap().unwrap();
    assert_eq!(*rescued.document, named("edited"));
    assert_eq!(rescued.name.as_deref(), Some("a.vrdp"));
    drop(entry);
    // Nothing more to move.
    let listed = block_on(list(
        &dir,
        Some(&store),
        || unreachable!(),
        &Downloads::default(),
        |_| false,
    ));
    assert!(!listed.rescued);
}

/// A design is read whole to download it, with the sum of its newest save
/// to record the download by.
#[test]
fn a_design_is_read_whole_to_download() {
    let (_temp, dir) = dir("saved-read");
    let mut design = saved(&dir, "a.vrdp", &named("one"));
    block_on(save(&dir, &mut design, &named("two"), &[])).unwrap();
    let (read, sum) = block_on(read_whole(&dir, "a.vrdp")).unwrap();
    assert_eq!(read, bytes(&dir, "a.vrdp"));
    assert_eq!(sum, design.tail().sum());
    assert!(block_on(read_whole(&dir, "b.vrdp")).is_err());
    // Recorded so, it's the latest downloaded till the next save.
    let mut downloads = Downloads::default();
    downloads.record("a.vrdp", UnixSeconds(1), Some(sum));
    drop(design);
    let listed = listing(&dir, &downloads, |_| false);
    assert_eq!(listed[0].download, DownloadStatus::Latest(UnixSeconds(1)));
}

/// Browser storage running out of room says so plainly.
#[test]
fn a_full_storage_says_so() {
    let full = FileError::Io(io::Error::from(io::ErrorKind::StorageFull));
    assert_eq!(save_error(full), SaveError::Failed(FULL.to_owned()));
    assert_eq!(save_error(FileError::Conflict), SaveError::Conflict);
}
