//! Entwürfe: erzeugte, noch nicht gespeicherte Passwörter, die nur im Arbeitsspeicher liegen.
//!
//! Sicherheitsmechanismus:
//! - Wird das Fenster minimiert, läuft ein Timer von [`WIPE_DELAY`]. Danach werden alle
//!   Entwürfe überschrieben und gelöscht. Wird das Fenster vorher wieder hergestellt,
//!   bricht der Timer ab.
//! - Beim Schließen werden die Entwürfe sofort überschrieben ([`Drafts::wipe`]).
//!
//! Die Zeit wird von außen übergeben, damit sich der Timer in Tests vorspulen lässt.

use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::generator::Options;

pub const WIPE_DELAY: Duration = Duration::from_secs(30);
/// Mehr Entwürfe hält die App nicht vor; der älteste fällt zuerst heraus.
pub const MAX_DRAFTS: usize = 20;

pub struct Draft {
    pub id: u64,
    pub password: Zeroizing<String>,
    /// Einstellungen, mit denen das Passwort erzeugt wurde. Sie werden mit dem Eintrag gespeichert.
    pub options: Options,
    pub created: Instant,
    /// Gespeichert heißt: eine Kopie liegt im Verlauf. Der Entwurf selbst bleibt
    /// nur noch als Anzeige des aktuellen Passworts im Speicher.
    pub saved: bool,
}

#[derive(Default)]
pub struct Drafts {
    items: Vec<Draft>,
    next_id: u64,
    hidden_since: Option<Instant>,
}

impl Drafts {
    pub fn push(&mut self, password: Zeroizing<String>, options: Options, now: Instant) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Draft {
            id,
            password,
            options,
            created: now,
            saved: false,
        });
        if self.items.len() > MAX_DRAFTS {
            self.items.remove(0);
        }
        id
    }

    /// Das zuletzt erzeugte Passwort.
    pub fn current(&self) -> Option<&Draft> {
        self.items.last()
    }

    /// Nicht gespeicherte Entwürfe, neueste zuerst.
    pub fn unsaved(&self) -> impl Iterator<Item = &Draft> {
        self.items.iter().rev().filter(|d| !d.saved)
    }

    pub fn unsaved_count(&self) -> usize {
        self.items.iter().filter(|d| !d.saved).count()
    }

    pub fn mark_saved(&mut self, id: u64) {
        if let Some(d) = self.items.iter_mut().find(|d| d.id == id) {
            d.saved = true;
        }
    }

    pub fn discard(&mut self, id: u64) {
        self.items.retain(|d| d.id != id);
    }

    pub fn password(&self, id: u64) -> Option<&str> {
        self.items
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.password.as_str())
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Das Fenster wurde minimiert: der Timer startet (einmalig, bis es wieder sichtbar ist).
    pub fn window_hidden(&mut self, now: Instant) {
        self.hidden_since.get_or_insert(now);
    }

    /// Das Fenster ist wieder sichtbar: der Timer bricht ab.
    pub fn window_visible(&mut self) {
        self.hidden_since = None;
    }

    /// Prüft den Timer. Gibt `true` zurück, wenn dabei Entwürfe gelöscht wurden.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(since) = self.hidden_since else {
            return false;
        };
        if now.saturating_duration_since(since) < WIPE_DELAY {
            return false;
        }
        self.hidden_since = None;
        let had_items = !self.items.is_empty();
        self.wipe();
        had_items
    }

    /// Löscht alle Entwürfe sofort. `Zeroizing` überschreibt beim Freigeben den Speicher.
    pub fn wipe(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(now: Instant) -> Drafts {
        let mut d = Drafts::default();
        d.push(Zeroizing::new("erstes-passwort".into()), Options::default(), now);
        d.push(Zeroizing::new("zweites-passwort".into()), Options::default(), now);
        d
    }

    #[test]
    fn wipes_after_30_seconds_minimized() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        d.window_hidden(t0);
        assert!(!d.tick(t0 + Duration::from_secs(29)));
        assert_eq!(d.unsaved_count(), 2);
        assert!(d.tick(t0 + Duration::from_secs(30)));
        assert!(d.is_empty());
    }

    #[test]
    fn restoring_the_window_cancels_the_timer() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        d.window_hidden(t0);
        d.window_visible();
        assert!(!d.tick(t0 + Duration::from_secs(60)));
        assert_eq!(d.unsaved_count(), 2);
    }

    #[test]
    fn repeated_hidden_reports_do_not_restart_the_timer() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        d.window_hidden(t0);
        // Die Oberfläche meldet „minimiert“ alle 500 ms erneut.
        for ms in (500..30_000).step_by(500) {
            d.window_hidden(t0 + Duration::from_millis(ms));
        }
        assert!(d.tick(t0 + Duration::from_secs(30)));
        assert!(d.is_empty());
    }

    #[test]
    fn nothing_happens_while_visible() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        assert!(!d.tick(t0 + Duration::from_secs(3600)));
        assert_eq!(d.unsaved_count(), 2);
    }

    #[test]
    fn wipe_on_close_is_immediate() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        d.wipe();
        assert!(d.is_empty());
        assert!(d.current().is_none());
    }

    #[test]
    fn saved_drafts_leave_the_unsaved_list_but_are_wiped_too() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        let id = d.current().unwrap().id;
        d.mark_saved(id);
        assert_eq!(d.unsaved_count(), 1);
        d.window_hidden(t0);
        assert!(d.tick(t0 + WIPE_DELAY));
        assert!(d.is_empty());
    }

    #[test]
    fn tick_on_empty_list_reports_nothing_wiped() {
        let t0 = Instant::now();
        let mut d = Drafts::default();
        d.window_hidden(t0);
        assert!(!d.tick(t0 + WIPE_DELAY));
    }

    #[test]
    fn oldest_draft_drops_out_after_the_limit() {
        let t0 = Instant::now();
        let mut d = Drafts::default();
        for i in 0..(MAX_DRAFTS + 5) {
            d.push(Zeroizing::new(format!("pw-{i}")), Options::default(), t0);
        }
        assert_eq!(d.unsaved_count(), MAX_DRAFTS);
        assert_eq!(d.unsaved().last().unwrap().password.as_str(), "pw-5");
    }

    #[test]
    fn discard_removes_one_draft() {
        let t0 = Instant::now();
        let mut d = filled(t0);
        let first = d.unsaved().last().unwrap().id;
        d.discard(first);
        assert_eq!(d.unsaved_count(), 1);
        assert!(d.password(first).is_none());
    }
}
