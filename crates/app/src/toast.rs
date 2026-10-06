//! The toast: a short message shown for a few seconds at the bottom of
//! the screen ([`varde_view::toast`]), such as a name being taken. One
//! shows at a time, a new one replacing it. While one shows, the app
//! subscribes to `platform::toast_ticks`, whose ticks take it down once
//! its time is up.

use std::time::Duration;

use iced::time::Instant;

/// How long a toast shows.
pub(crate) const TOAST_TIME: Duration = Duration::from_secs(3);

/// The toast showing, if one is.
#[derive(Debug, Default)]
pub(crate) struct Toast {
    shown: Option<(String, Instant)>,
}

impl Toast {
    /// Shows `message` from `now` for [`TOAST_TIME`], in place of any
    /// toast showing.
    pub(crate) fn show(&mut self, message: String, now: Instant) {
        let until = now.checked_add(TOAST_TIME).unwrap_or(now);
        self.shown = Some((message, until));
    }

    /// Takes the toast down if its time is up at `now`.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.shown.take_if(|(_, until)| now >= *until);
    }

    /// The message showing, if one is.
    pub(crate) fn message(&self) -> Option<&str> {
        self.shown.as_ref().map(|(message, _)| message.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toast shows until its time is up, and a new one replaces it,
    /// showing its own time.
    #[test]
    fn a_toast_shows_for_its_time() {
        let mut toast = Toast::default();
        let now = Instant::now();
        toast.show("One".into(), now);
        toast.tick(now + TOAST_TIME / 2);
        assert_eq!(toast.message(), Some("One"));
        toast.show("Two".into(), now + TOAST_TIME / 2);
        toast.tick(now + TOAST_TIME);
        assert_eq!(toast.message(), Some("Two"));
        toast.tick(now + TOAST_TIME * 2);
        assert_eq!(toast.message(), None);
    }
}
