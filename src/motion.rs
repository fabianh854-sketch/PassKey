//! Bewegung nach den Kiesel-Regeln.
//!
//! - Hover und Farbwechsel: `duration-fast` (120 ms), Schalter, Tabs, Slider: `duration-base` (200 ms),
//!   Fortschrittsbalken: `duration-slow` (400 ms), Kurve `ease-out`.
//! - Nichts bewegt sich von selbst: Bilder werden nur angefordert, solange etwas animiert.
//! - Ist in Windows „Animationen anzeigen“ aus, springen alle Zustände sofort um.
//!
//! iced animiert Widget-Zustände nicht selbst. Die App hält deshalb hier pro Element einen
//! Zustand (`Animation<bool>`) und die Ansicht liest daraus einen Wert zwischen 0 und 1.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::animation::{Animation, Easing};

/// Alle Elemente, die einen eigenen Bewegungszustand haben.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Id {
    Profile,
    Minimize,
    Close,
    Generate,
    Copy,
    Save,
    Unlock,
    Eye,
    Lock,
    Reveal,
    CreateVault,
    TabGenerator,
    TabVault,
    Slider,
    Toggle(u8),
    CopyDraft(u64),
    DiscardDraft(u64),
    CopySaved(usize),
    Field(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// Der Mauszeiger liegt über dem Element.
    Hover(Id),
    /// Das Element ist an oder ausgewählt (Schalter, aktiver Tab).
    On(Id),
}

fn make(key: Key, initial: bool) -> Animation<bool> {
    let (ms, easing) = match key {
        Key::Hover(_) => (120, Easing::EaseInOut),
        Key::On(_) => (200, Easing::EaseOutQuart),
    };
    Animation::new(initial)
        .duration(Duration::from_millis(ms))
        .easing(easing)
}

/// Weiches Scrollen: Das Mausrad setzt ein Ziel, die Ansicht läuft in Bildschritten darauf zu.
pub struct Scroll {
    pub current: f32,
    pub target: f32,
    /// Größtmöglicher Versatz. Unbekannt (`f32::MAX`), bis iced die Inhaltshöhe meldet.
    pub max: f32,
    pub active: bool,
    pub last_frame: Option<Instant>,
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            current: 0.0,
            target: 0.0,
            max: f32::MAX,
            active: false,
            last_frame: None,
        }
    }
}

impl Scroll {
    /// Zurück an den Anfang, etwa beim Wechsel des Tabs.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Das Mausrad schiebt das Ziel um `delta` Pixel.
    pub fn wheel(&mut self, delta: f32, instant: bool) {
        self.target = (self.target + delta).clamp(0.0, self.max);
        if instant {
            self.current = self.target;
            self.active = false;
        } else {
            self.active = true;
        }
    }

    /// Ein Bildschritt. Gibt `true` zurück, wenn der Versatz neu gesetzt werden muss.
    pub fn step(&mut self, at: Instant) -> bool {
        if !self.active {
            return false;
        }
        // Zeitbasiert, damit es auf 60 und 144 Hz gleich schnell läuft.
        let dt = self
            .last_frame
            .map_or(0.016, |l| at.saturating_duration_since(l).as_secs_f32())
            .min(0.05);
        self.last_frame = Some(at);
        let k = 1.0 - (-dt / 0.07).exp();
        self.current += (self.target - self.current) * k;
        if (self.target - self.current).abs() < 0.5 {
            self.current = self.target;
            self.active = false;
            self.last_frame = None;
        }
        true
    }

    /// iced meldet Versatz und Inhaltsgröße. Aus dem Versatz übernehmen wir nur, wenn gerade nichts läuft
    /// (zum Beispiel beim Ziehen der Scrollleiste).
    pub fn report(&mut self, offset: f32, max: f32) {
        self.max = max;
        self.target = self.target.min(max);
        self.current = self.current.min(max);
        if !self.active {
            self.current = offset;
            self.target = offset;
        }
    }
}

pub struct Motion {
    reduced: bool,
    forced_reduced: bool,
    flags: HashMap<Key, Animation<bool>>,
    strength: Animation<f32>,
    pub scroll: Scroll,
}

impl Motion {
    pub fn new(forced_reduced: bool) -> Self {
        Self {
            reduced: forced_reduced || system_prefers_reduced_motion(),
            forced_reduced,
            flags: HashMap::new(),
            strength: Animation::new(0.0_f32)
                .duration(Duration::from_millis(400))
                .easing(Easing::EaseOutQuart),
            scroll: Scroll::default(),
        }
    }

    pub fn reduced(&self) -> bool {
        self.reduced
    }

    /// Wird regelmäßig aufgerufen, damit eine Änderung in den Windows-Einstellungen greift.
    pub fn refresh_reduced(&mut self) {
        self.reduced = self.forced_reduced || system_prefers_reduced_motion();
    }

    /// Setzt den Anfangszustand ohne Übergang.
    pub fn init_on(&mut self, id: Id, value: bool) {
        let key = Key::On(id);
        self.flags.insert(key, make(key, value));
    }

    pub fn init_strength(&mut self, value: f32) {
        self.strength = Animation::new(value)
            .duration(Duration::from_millis(400))
            .easing(Easing::EaseOutQuart);
    }

    pub fn set(&mut self, key: Key, value: bool, now: Instant) {
        let anim = self.flags.entry(key).or_insert_with(|| make(key, false));
        if anim.value() != value {
            anim.go_mut(value, now);
        }
    }

    pub fn set_strength(&mut self, value: f32, now: Instant) {
        if (self.strength.value() - value).abs() > f32::EPSILON {
            self.strength.go_mut(value, now);
        }
    }

    /// 0.0 bis 1.0. Bei reduzierter Bewegung ohne Übergang.
    pub fn amount(&self, key: Key, now: Instant) -> f32 {
        match self.flags.get(&key) {
            None => 0.0,
            Some(a) if self.reduced => f32::from(a.value()),
            Some(a) => a.interpolate(0.0, 1.0, now),
        }
    }

    pub fn strength(&self, now: Instant) -> f32 {
        if self.reduced {
            self.strength.value()
        } else {
            self.strength.interpolate_with(|v| v, now)
        }
    }

    /// Läuft gerade etwas? Nur dann fordert die App Bildschritte an.
    pub fn animating(&self, now: Instant) -> bool {
        self.scroll.active
            || (!self.reduced
                && (self.strength.is_animating(now)
                    || self.flags.values().any(|a| a.is_animating(now))))
    }
}

/// Werte für die Ansicht, zu einem festen Zeitpunkt gelesen.
pub struct Fx<'a> {
    motion: &'a Motion,
    now: Instant,
}

impl<'a> Fx<'a> {
    pub fn new(motion: &'a Motion, now: Instant) -> Self {
        Self { motion, now }
    }

    pub fn hover(&self, id: Id) -> f32 {
        self.motion.amount(Key::Hover(id), self.now)
    }

    pub fn on(&self, id: Id) -> f32 {
        self.motion.amount(Key::On(id), self.now)
    }

    pub fn strength(&self) -> f32 {
        self.motion.strength(self.now)
    }
}

/// Windows: Einstellungen > Barrierefreiheit > Visuelle Effekte > Animationseffekte.
#[cfg(windows)]
pub fn system_prefers_reduced_motion() -> bool {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, flags: u32) -> i32;
    }
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;

    let mut enabled: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };
    ok != 0 && enabled == 0
}

#[cfg(not(windows))]
pub fn system_prefers_reduced_motion() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_fades_in_and_settles() {
        let t0 = Instant::now();
        let mut m = Motion::new(false);
        let key = Key::Hover(Id::Generate);
        assert_eq!(m.amount(key, t0), 0.0);
        m.set(key, true, t0);
        let mid = m.amount(key, t0 + Duration::from_millis(60));
        assert!(mid > 0.0 && mid < 1.0, "Zwischenwert erwartet, war {mid}");
        assert!(m.animating(t0 + Duration::from_millis(60)));
        assert_eq!(m.amount(key, t0 + Duration::from_millis(500)), 1.0);
        assert!(!m.animating(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn reduced_motion_jumps_to_the_end() {
        let t0 = Instant::now();
        let mut m = Motion::new(true);
        let key = Key::On(Id::Toggle(0));
        m.set(key, true, t0);
        assert_eq!(m.amount(key, t0), 1.0);
        assert!(!m.animating(t0));
    }

    #[test]
    fn nothing_animates_while_idle() {
        let m = Motion::new(false);
        assert!(!m.animating(Instant::now()));
    }

    #[test]
    fn smooth_scroll_runs_towards_target_and_stops() {
        let t0 = Instant::now();
        let mut s = Scroll::default();
        s.max = 500.0;
        s.wheel(120.0, false);
        assert!(s.active);
        let mut at = t0;
        let mut last = 0.0;
        for _ in 0..200 {
            at += Duration::from_millis(16);
            s.step(at);
            assert!(s.current >= last, "Versatz darf nicht zurückspringen");
            assert!(s.current <= 120.0, "kein Überschwingen");
            last = s.current;
            if !s.active {
                break;
            }
        }
        assert_eq!(s.current, 120.0);
        assert!(!s.active);
    }

    #[test]
    fn scroll_target_is_clamped() {
        let mut s = Scroll::default();
        s.max = 100.0;
        s.wheel(500.0, false);
        assert_eq!(s.target, 100.0);
        s.wheel(-900.0, false);
        assert_eq!(s.target, 0.0);
    }

    #[test]
    fn scroll_with_reduced_motion_is_instant() {
        let mut s = Scroll::default();
        s.max = 300.0;
        s.wheel(90.0, true);
        assert_eq!(s.current, 90.0);
        assert!(!s.active);
    }
}
