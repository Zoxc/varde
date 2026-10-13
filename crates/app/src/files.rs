//! The files side of the app, which both screens' steps use: the IO
//! lane, downloads, the lists the welcome screen shows and their
//! thumbnails.

use std::path::{Path, PathBuf};

use varde_io::{BrowserDesign, Panic, Recovered, Request as IoRequest, SaveError};
use varde_view::Mode;

use crate::doc::Downloader;
use crate::io::Io;
use crate::recent::Recent;
use crate::{samples, when};

/// The files side of the app, which both screens' steps use: the IO lane,
/// downloads, and the lists the welcome screen shows.
pub(crate) struct Files {
    pub(crate) io: Io,
    /// How a design is handed over as a download: on the web, by the page.
    /// `None` natively, where there's nothing to download. Tests set it
    /// natively.
    pub(crate) downloader: Option<Downloader>,
    /// Whether designs are saved in browser storage, by name, as on the
    /// web: Save As asks for a name in the app's own dialog. Tests set it
    /// natively.
    pub(crate) browser_storage: bool,
    /// Whether the File System Access API is there (Chromium): files of
    /// the user's are picked and saved back to, Save As offers one, and
    /// exports aren't downloaded.
    pub(crate) file_system_access: bool,
    pub(crate) recent: Recent,
    /// The units a new design starts in: the user's setting, kept in
    /// step with it by `Varde::update`.
    pub(crate) new_units: varde_document::LengthUnit,
    /// New designs left behind by sessions that crashed, newest first.
    pub(crate) recovered: Vec<Recovered>,
    /// On the web, the designs saved in browser storage, newest first, as
    /// last listed, and their thumbnails as iced handles, made once per
    /// design and save, see [`Files::browser_listed`].
    pub(crate) browser: Vec<BrowserDesign>,
    pub(crate) browser_thumbnails: Vec<BrowserThumbnail>,
    /// The sample designs' thumbnails ([`samples::SAMPLES`]), each as its
    /// file has one, made once.
    pub(crate) sample_thumbnails: Vec<Option<ThumbnailHandles>>,
    /// On the web, what the browser says of its storage, see
    /// [`varde_io::storage`].
    pub(crate) storage: StorageState,
    /// Whether to ask the browser to keep its storage for good, as the
    /// user just saved to it: done after the step, see `Varde::update`.
    pub(crate) ask_persist: bool,
    /// The recent files' thumbnails, by path, as last read: see
    /// [`Files::load_thumbnails`].
    pub(crate) thumbnails: Vec<(PathBuf, ThumbnailHandles)>,
    /// The panic a session recorded, see [`varde_io::panicked`], till the
    /// user discards it.
    pub(crate) panic: Option<Panic>,
}

/// The thumbnail of a design in browser storage, as an iced handle, and
/// the design's name and the sum of its newest save, which tell when it's
/// another.
pub(crate) struct BrowserThumbnail {
    pub(crate) name: String,
    pub(crate) sum: u128,
    pub(crate) handles: ThumbnailHandles,
}

/// A design's thumbnail as iced handles, one for each theme's image, made
/// once: a handle is uploaded once per id, the first time it's shown.
#[derive(Clone)]
pub(crate) struct ThumbnailHandles {
    pub(crate) light: iced::widget::image::Handle,
    pub(crate) dark: iced::widget::image::Handle,
}

impl ThumbnailHandles {
    pub(crate) fn new(thumbnail: varde_io::thumbnail::Thumbnail) -> ThumbnailHandles {
        let handle = |image: varde_io::thumbnail::Image| {
            let (width, height) = (image.width(), image.height());
            iced::widget::image::Handle::from_rgba(width, height, image.into_rgba())
        };
        ThumbnailHandles {
            light: handle(thumbnail.light),
            dark: handle(thumbnail.dark),
        }
    }

    /// The handle of the image in `mode`'s colours.
    pub(crate) fn of(&self, mode: Mode) -> iced::widget::image::Handle {
        match mode {
            Mode::Light => self.light.clone(),
            Mode::Dark => self.dark.clone(),
        }
    }
}

/// What the platform offers the document screen, as [`Files`] knows it:
/// the file menu offers Download where a design can be downloaded, and
/// the Save As dialog a file on the computer where the File System Access
/// API is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Offers {
    pub(crate) download: bool,
    pub(crate) file_system_access: bool,
}

/// What the browser says of its storage, on the web.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StorageState {
    /// Whether it keeps the site's storage for good, if it says.
    pub(crate) persisted: Option<bool>,
    /// How much of it the site uses, and may, if it says.
    pub(crate) space: Option<varde_io::storage::Space>,
    /// Whether the app asked it to keep the site's storage this session.
    pub(crate) asked: bool,
}

impl Files {
    pub(crate) fn new(downloader: Option<Downloader>) -> Self {
        Self {
            io: Io::new(),
            downloader,
            browser_storage: cfg!(target_arch = "wasm32"),
            file_system_access: varde_io::pick::file_system_access(),
            recent: Recent::default(),
            new_units: varde_document::LengthUnit::default(),
            recovered: Vec::new(),
            browser: Vec::new(),
            browser_thumbnails: Vec::new(),
            sample_thumbnails: (samples::SAMPLES.iter())
                .map(|sample| varde_io::thumbnail::of_file(sample.file).map(ThumbnailHandles::new))
                .collect(),
            storage: StorageState::default(),
            ask_persist: false,
            thumbnails: Vec::new(),
            panic: None,
        }
    }

    /// Discards the new design at `path` left behind by a crash, gone
    /// from the list at once: the lane lists what's left after.
    pub(crate) fn discard_recovered(&mut self, path: PathBuf) {
        self.recovered.retain(|design| design.path != path);
        self.io.send(IoRequest::DiscardRecovered { path });
    }

    /// Deletes the design `name` in browser storage, gone from the list at
    /// once: the lane lists what's left after.
    pub(crate) fn delete_from_browser(&mut self, name: String) {
        self.browser.retain(|design| design.name != name);
        self.io.send(IoRequest::DeleteFromBrowser { name });
    }

    /// Takes the designs in browser storage as listed. Their thumbnails'
    /// handles are kept while the design is saved as it was, by its sum:
    /// a handle is uploaded once per id.
    pub(crate) fn browser_listed(&mut self, designs: Vec<BrowserDesign>) {
        let mut kept = std::mem::take(&mut self.browser_thumbnails);
        self.browser_thumbnails = (designs.iter())
            .filter_map(|design| {
                let (thumbnail, sum) = (design.thumbnail.as_ref()?, design.sum?);
                let same = |thumbnail: &BrowserThumbnail| {
                    thumbnail.name == design.name && thumbnail.sum == sum
                };
                let handles = match kept.iter().position(same) {
                    Some(at) => kept.swap_remove(at).handles,
                    None => ThumbnailHandles::new(thumbnail.clone()),
                };
                Some(BrowserThumbnail {
                    name: design.name.clone(),
                    sum,
                    handles,
                })
            })
            .collect();
        self.browser = designs;
    }

    /// The thumbnail of `design`, listed in browser storage, if it has
    /// one, in `mode`'s colours.
    pub(crate) fn browser_thumbnail(
        &self,
        design: &BrowserDesign,
        mode: Mode,
    ) -> Option<iced::widget::image::Handle> {
        (self.browser_thumbnails.iter())
            .find(|thumbnail| Some(thumbnail.sum) == design.sum && thumbnail.name == design.name)
            .map(|thumbnail| thumbnail.handles.of(mode))
    }

    /// The thumbnail of the sample `index` ([`samples::SAMPLES`]), if its
    /// file has one, in `mode`'s colours.
    pub(crate) fn sample_thumbnail(
        &self,
        index: usize,
        mode: Mode,
    ) -> Option<iced::widget::image::Handle> {
        let handles = self.sample_thumbnails.get(index)?.as_ref()?;
        Some(handles.of(mode))
    }

    /// What the platform offers the document screen.
    pub(crate) fn offers(&self) -> Offers {
        Offers {
            download: self.downloader.is_some(),
            file_system_access: self.file_system_access,
        }
    }

    /// Asks the lane for the designs in browser storage, where there's
    /// that, and the browser for what it says of it.
    pub(crate) fn list_browser(&mut self) {
        if self.browser_storage {
            self.io.send(IoRequest::ListBrowser);
        }
    }

    /// Downloads `bytes`, a design's file named `name` in browser
    /// storage, with the page's downloader, and lists the storage again
    /// with the download recorded. What to tell the user, if the download
    /// failed or couldn't be recorded (`not_recorded` says why).
    pub(crate) fn download(
        &mut self,
        name: &str,
        bytes: &[u8],
        not_recorded: Option<String>,
    ) -> Option<String> {
        let result = match &self.downloader {
            Some(download) => download(name, bytes),
            None => Err("there's nowhere to download to".to_owned()),
        };
        self.list_browser();
        match (result, not_recorded) {
            (Err(error), _) => Some(error),
            (Ok(()), Some(why)) => Some(format!(
                "Downloaded {name}, but couldn't record the download: {why}"
            )),
            (Ok(()), None) => None,
        }
    }

    /// Whether to ask the browser to keep its storage for good now: once a
    /// session, as the user saves to it, unless it does already.
    pub(crate) fn take_ask_persist(&mut self) -> bool {
        let ask = std::mem::take(&mut self.ask_persist)
            && !self.storage.asked
            && self.storage.persisted != Some(true);
        self.storage.asked |= ask;
        ask
    }

    /// Takes the answer to a Save As whose document has gone since: the
    /// file it made is closed, as there's nothing left to write it.
    pub(crate) fn settle_saved_as(&mut self, result: Result<varde_io::SavedAs, SaveError>) {
        if let Ok(saved) = result {
            self.io.close_clean(saved.file);
        }
    }

    /// `path` as the file cell's tooltip shows it, the home directory as
    /// `~`.
    pub(crate) fn shown_path(&self, path: &Path) -> String {
        let dir = self.recent.display_dir(path);
        let file = path.file_name().unwrap_or_default();
        Path::new(&dir).join(file).display().to_string()
    }

    /// Records `path` as opened just now in the recent files list, and
    /// stores the list once it's complete.
    pub(crate) fn remember(&mut self, path: PathBuf) {
        self.io.send_all(self.recent.opened(path, when::now()));
    }

    /// Asks the lane for the recent files' thumbnails, shown on the
    /// welcome screen once they arrive: as the list loads, and as the
    /// welcome screen shows again, a design maybe saved since.
    pub(crate) fn load_thumbnails(&mut self) {
        let paths: Vec<_> = (self.recent.entries().iter())
            .map(|listed| listed.entry.path.clone())
            .collect();
        if crate::welcome::RECENT_FILES && !paths.is_empty() {
            self.io.send(IoRequest::LoadThumbnails { paths });
        }
    }

    /// The thumbnail of the recent file at `path`, if it has one, in
    /// `mode`'s colours.
    pub(crate) fn thumbnail(&self, path: &Path, mode: Mode) -> Option<iced::widget::image::Handle> {
        (self.thumbnails.iter())
            .find(|(at, _)| at == path)
            .map(|(_, handles)| handles.of(mode))
    }
}
