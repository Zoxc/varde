//! The file menu: saving, exporting, the design's units and tolerance.

use iced::widget::{Space, column, container, mouse_area, opaque, row, text};
use iced::{Alignment, Element, Length, Padding, mouse};
use varde_document::Tolerance;
use varde_expr::LengthUnit;

use crate::icons::{self, Icon};
use crate::menu::{MENU_ITEM_SPACING, menu_item, menu_separator, ticked};
use crate::shortcut::{Binding, file_bindings};
use crate::theme;
use crate::toolbar::TOOLBAR_HEIGHT;
use crate::{Downloads, Edit, File, Look, Message};

/// The design units the file menu offers, with their names.
const UNITS: [(LengthUnit, &str); 2] =
    [(LengthUnit::Mm, "Millimetres"), (LengthUnit::In, "Inches")];

/// The tolerances the file menu offers, by fit tolerance in mm, with
/// their names.
const TOLERANCES: [(f64, &str); 3] = [(1e-4, "0.1 µm"), (1e-3, "1 µm"), (1e-2, "10 µm")];

/// The name of the fit tolerance `fit`, in mm, in micrometres to three
/// decimals without trailing zeros: "0.05 µm", "25 µm".
pub fn tolerance_label(fit: f64) -> String {
    let mut text = format!("{:.3}", fit * 1000.0);
    let kept = text.trim_end_matches('0').trim_end_matches('.').len();
    text.truncate(kept);
    format!("{text} µm")
}

/// The tolerances the file menu lists for a design of `tolerance`, by
/// fit tolerance in mm, with their names and whether they're ticked: the
/// ones offered, and `tolerance` unticked after them if it isn't one,
/// under a name none of them has.
fn tolerance_choices(tolerance: Tolerance) -> Vec<(f64, String, bool)> {
    let fit = tolerance.fit();
    let listed = TOLERANCES.iter().any(|&(offered, _)| offered == fit);
    let offered = TOLERANCES.map(|(offered, label)| (offered, label.to_owned(), offered == fit));
    let other = (!listed).then(|| {
        // Rounded as it's named, it may look like one offered: then it's
        // named exactly, in millimetres.
        let label = tolerance_label(fit);
        let taken = TOLERANCES.iter().any(|&(_, offered)| offered == label);
        let label = if taken { format!("{fit} mm") } else { label };
        (fit, label, false)
    });
    offered.into_iter().chain(other).collect()
}

/// What the file menu shows, see [`file_menu`].
pub struct FileMenu {
    pub editable: bool,
    pub edited: bool,
    pub exportable: bool,
    pub units: LengthUnit,
    pub tolerance: Tolerance,
    /// Where it stands against its downloads, for a design in browser
    /// storage: the menu starts with it.
    pub downloads: Option<Downloads>,
    /// Whether it offers Download: on the web.
    pub downloadable: bool,
    /// For a design in browser storage, it offers Rename…: whether the
    /// design may be renamed now.
    pub rename: Option<bool>,
}

/// The file menu, as a layer over the whole screen. Clicking outside the
/// menu closes it. Save, and changing the design's `units` or its
/// `tolerance`, are disabled unless the document is `editable`, and Save
/// unless it's `edited` too. Export 3MF is disabled unless `exportable`.
/// A tolerance the menu doesn't offer, from a file, shows unticked. On
/// the web it offers Download, and for a design in browser storage
/// Rename…, starting with where it stands against its downloads.
pub fn file_menu(state: FileMenu) -> Element<'static, Message> {
    let FileMenu {
        editable,
        edited,
        exportable,
        units,
        tolerance,
        downloads,
        downloadable,
        rename,
    } = state;
    let item = menu_item;
    let separator = menu_separator;

    let bound = |icon, label: &'static str, binding: Binding| {
        let message = binding.sends();
        item(icon, label.into(), binding.shortcut, message)
    };

    let [save, save_as] = file_bindings(editable, edited);
    // No key: exporting is rare, and the tool rail has no tool for it.
    let export = exportable.then_some(Message::File(File::Export));
    let rename = rename.map(|enabled| {
        item(
            Icon::Rename,
            "Rename…".into(),
            None,
            enabled.then_some(Message::File(File::Rename)),
        )
    });
    let download = downloadable.then(|| {
        item(
            Icon::Download,
            "Download".into(),
            None,
            Some(Message::File(File::Download)),
        )
    });
    // Its dot in line with the items' icons.
    let downloads = downloads.as_ref().map(|downloads| {
        let (said, color) = crate::welcome::downloads_said(downloads);
        column![
            container(
                row![
                    container(container(Space::new().width(7).height(7)).style(theme::dot(color)))
                        .center_x(icons::INLINE),
                    text(said).size(11.5).style(theme::muted_text),
                ]
                .spacing(MENU_ITEM_SPACING)
                .align_y(Alignment::Center),
            )
            .padding([6, 8]),
            separator(),
        ]
    });
    let saving = column![
        downloads,
        bound(Icon::Save, "Save", save),
        bound(Icon::Save, "Save As…", save_as),
        rename,
        download,
        separator(),
        item(Icon::Export, "Export 3MF…".into(), None, export),
        separator(),
    ];
    // A new design starts in millimetres; its units are chosen here, and
    // changed here later.
    let heading = |label| container(crate::chrome::heading(label)).padding([4, 8]);
    let choices = UNITS.map(|(unit, label)| {
        let message = editable.then_some(Message::Edit(Edit::SetUnits(unit)));
        item(ticked(unit == units), label.into(), None, message).into()
    });
    // How closely curved surfaces are fitted, which is rarely changed.
    let tolerances = tolerance_choices(tolerance)
        .into_iter()
        .map(|(fit, label, on)| {
            let message = Tolerance::new(fit)
                .filter(|_| editable)
                .map(|tolerance| Message::Edit(Edit::SetTolerance(tolerance)));
            item(ticked(on), label.into(), None, message).into()
        });
    let menu = container(
        column![
            saving,
            heading("Units"),
            column(choices),
            heading("Tolerance"),
            column(tolerances),
            separator(),
            item(
                Icon::Close,
                "Close document".into(),
                None,
                Some(Message::File(File::CloseDocument))
            ),
        ]
        .width(232),
    )
    .padding(4)
    .style(theme::menu);

    mouse_area(
        container(opaque(menu))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO.top(TOOLBAR_HEIGHT + 4.0).left(8)),
    )
    .interaction(mouse::Interaction::Idle)
    .on_press(Message::Look(Look::CloseFileMenu))
    .on_right_press(Message::Look(Look::CloseFileMenu))
    .into()
}

#[cfg(test)]
mod tests {
    use iced::advanced::{Layout, Shell, clipboard};
    use iced::{Event, Point, Rectangle, Size, mouse};
    use varde_document::Tolerance;
    use varde_expr::LengthUnit;

    use super::{FileMenu, TOLERANCES, file_menu, tolerance_choices, tolerance_label};
    use crate::testing::Laid;
    use crate::{Downloads, File, Message};

    /// The file menu as natively, `exportable` or not.
    fn native_menu(exportable: bool) -> FileMenu {
        FileMenu {
            editable: true,
            edited: true,
            exportable,
            units: LengthUnit::Mm,
            tolerance: Tolerance::DEFAULT,
            downloads: None,
            downloadable: false,
            rename: None,
        }
    }

    /// What clicking the file menu's item `label` sends, the menu laid out
    /// over a 800 × 600 window, `exportable` or not.
    fn click_file_menu(label: &str, exportable: bool) -> Vec<Message> {
        click_in_menu(native_menu(exportable), label)
    }

    /// What clicking the item `label` of the file menu `menu` sends.
    fn click_in_menu(menu: FileMenu, label: &str) -> Vec<Message> {
        let window = Size::new(800.0, 600.0);
        let menu = file_menu(menu);
        let mut laid = Laid::new(menu, window);
        let shown = laid.texts();
        let item = shown
            .iter()
            .find(|shown| shown.text == label)
            .unwrap_or_else(|| panic!("no {label:?} in {shown:?}"));
        let at = item.bounds.center();
        let mut messages = Vec::new();
        for event in [mouse::Event::ButtonPressed, mouse::Event::ButtonReleased] {
            let mut shell = Shell::new(&mut messages);
            laid.element.as_widget_mut().update(
                &mut laid.tree,
                &Event::Mouse(event(mouse::Button::Left)),
                Layout::new(&laid.node),
                mouse::Cursor::Available(Point::new(at.x, at.y)),
                &laid.renderer,
                &mut clipboard::Null,
                &mut shell,
                &Rectangle::with_size(window),
            );
        }
        messages
    }

    /// On the web the menu offers Download and, for a design in browser
    /// storage, starts with where it stands against its downloads and
    /// offers Rename…; natively none of that. Where the design is kept is
    /// the bar under the file cell's to say, not the menu's.
    #[test]
    fn on_the_web_the_file_menu_says_where_the_design_stands_against_its_downloads() {
        let web = || FileMenu {
            downloads: Some(Downloads::Latest(Some("just now".to_owned()))),
            downloadable: true,
            rename: Some(true),
            ..native_menu(false)
        };
        let shown = crate::testing::Laid::new(file_menu(web()), Size::new(800.0, 600.0)).texts();
        let first = shown
            .iter()
            .min_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
        assert_eq!(
            first.map(|shown| shown.text.as_str()),
            Some("Latest downloaded just now"),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|shown| shown.text == "In browser storage"));
        let sent = click_in_menu(web(), "Download");
        assert!(
            matches!(sent[..], [Message::File(File::Download)]),
            "{sent:?}"
        );
        let sent = click_in_menu(web(), "Rename…");
        assert!(
            matches!(sent[..], [Message::File(File::Rename)]),
            "{sent:?}"
        );
        // Not while it may not be renamed, say as it's saved.
        let saving = FileMenu {
            rename: Some(false),
            ..web()
        };
        assert!(click_in_menu(saving, "Rename…").is_empty());
        let computer = FileMenu {
            downloads: None,
            rename: None,
            ..web()
        };
        let shown = crate::testing::Laid::new(file_menu(computer), Size::new(800.0, 600.0)).texts();
        let first = shown
            .iter()
            .min_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
        assert_eq!(
            first.map(|shown| shown.text.as_str()),
            Some("Save"),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|shown| shown.text == "Rename…"));
        let shown =
            crate::testing::Laid::new(file_menu(native_menu(true)), Size::new(800.0, 600.0))
                .texts();
        for text in ["Download", "Rename…"] {
            assert!(
                !shown.iter().any(|shown| shown.text == text),
                "{text:?} natively"
            );
        }
    }

    #[test]
    fn export_is_in_the_file_menu_while_there_is_something_to_export() {
        let sent = click_file_menu("Export 3MF…", true);
        assert!(
            matches!(sent[..], [Message::File(File::Export)]),
            "{sent:?}"
        );
        // Disabled, the click lands on the menu, which keeps it.
        let sent = click_file_menu("Export 3MF…", false);
        assert!(sent.is_empty(), "{sent:?}");
        // The items around it still go.
        let sent = click_file_menu("Save As…", false);
        assert!(
            matches!(sent[..], [Message::File(File::SaveAs)]),
            "{sent:?}"
        );
    }

    #[test]
    fn a_tolerance_not_offered_is_listed_unticked_under_its_own_name() {
        let near = f64::from_bits(1e-3_f64.to_bits() + 1);
        for fit in [5e-5, near, 1.0004e-3, 0.0999999] {
            let choices = tolerance_choices(Tolerance::new(fit).unwrap());
            assert_eq!(choices.len(), TOLERANCES.len() + 1, "{fit}");
            assert!(choices.iter().all(|&(.., on)| !on), "{fit}");
            let (last, label, _) = choices.last().unwrap();
            assert_eq!(*last, fit);
            assert!(
                TOLERANCES.iter().all(|&(_, offered)| offered != label),
                "{fit} is named {label}, as an offered one is"
            );
        }
        let choices = tolerance_choices(Tolerance::DEFAULT);
        assert_eq!(choices.len(), TOLERANCES.len());
        let ticked: Vec<f64> = choices.iter().filter(|c| c.2).map(|c| c.0).collect();
        assert_eq!(ticked, [Tolerance::DEFAULT.fit()]);
    }

    #[test]
    fn tolerances_are_named_in_micrometres() {
        for (fit, label) in TOLERANCES {
            assert!(Tolerance::new(fit).is_some(), "{label}");
            assert_eq!(tolerance_label(fit), label);
        }
        assert_eq!(tolerance_label(Tolerance::MIN_FIT), "0.01 µm");
        assert_eq!(tolerance_label(Tolerance::MAX_FIT), "100 µm");
        assert_eq!(tolerance_label(0.025), "25 µm");
        assert_eq!(tolerance_label(5e-5), "0.05 µm");
    }
}
