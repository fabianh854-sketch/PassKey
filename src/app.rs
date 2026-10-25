//! Zustand und Ablauf der App.
//!
//! Der Verlauf liegt verschlüsselt in einer Datei (siehe [`crate::store`]). Entsperren und
//! Anlegen leiten den Schlüssel mit Argon2id ab. Das dauert spürbar, deshalb läuft es in einem
//! eigenen Thread, und die Oberfläche bleibt bedienbar.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::futures::channel::oneshot;
use iced::widget::operation::{scroll_to, AbsoluteOffset};
use iced::widget::{column, container, mouse_area, row, scrollable, stack, Space};
use iced::{mouse, system, window, Background, Element, Length, Subscription, Task, Theme};
use zeroize::{Zeroize, Zeroizing};

use crate::drafts::Drafts;
use crate::generator::{self, Options};
use crate::motion::{Fx, Id, Key, Motion};
use crate::store::{self, KdfParams, Vault};
use crate::theme::{self, Tokens, DISPLAY};
use crate::ui::widgets::{self, Icons, Variant};

pub const MIN_MASTER_LEN: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Generator,
    Vault,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaultState {
    /// Es gibt noch keine Datei: Ersteinrichtung.
    NoFile,
    Locked,
    Unlocked,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupError {
    TooShort,
    Mismatch,
    /// Die Datei ließ sich nicht schreiben.
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    Copied,
    Saved,
    SaveFailed,
}

/// Bringt eine geöffnete Datei aus dem Hintergrund-Thread in eine Nachricht. Nachrichten
/// müssen `Clone` sein, die entsperrte Datei soll es aber nur einmal geben: `take` holt sie heraus.
#[derive(Clone)]
pub struct Handoff(Arc<Mutex<Option<Result<Vault, store::Error>>>>);

impl Handoff {
    fn take(&self) -> Option<Result<Vault, store::Error>> {
        self.0.lock().ok()?.take()
    }
}

impl std::fmt::Debug for Handoff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handoff")
    }
}

/// Führt `job` in einem eigenen Thread aus. Ein Thread statt des Executors, weil Argon2 rechnet
/// statt zu warten und den Takt der Oberfläche sonst blockieren würde.
fn in_background(
    job: impl FnOnce() -> Result<Vault, store::Error> + Send + 'static,
) -> impl Future<Output = Handoff> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    async move {
        let result = rx.await.unwrap_or(Err(store::Error::Internal));
        Handoff(Arc::new(Mutex::new(Some(result))))
    }
}

pub struct App {
    pub icons: Icons,
    choice: ThemeChoice,
    system_mode: iced::theme::Mode,

    pub tab: Tab,
    pub opts: Options,
    pub label: String,
    pub drafts: Drafts,
    pub notice: Option<Notice>,
    /// Entwürfe wurden gelöscht (Timer): Hinweis in der Ansicht Erzeugen.
    pub wiped_notice: bool,
    pub now: Instant,
    pub motion: Motion,

    pub vault: VaultState,
    vault_path: PathBuf,
    /// Die entsperrte Datei. `None`, solange gesperrt; Fallenlassen überschreibt Schlüssel und Einträge.
    store: Option<Vault>,
    /// Argon2 läuft gerade (Entsperren oder Anlegen).
    pub busy: bool,
    pub master_input: String,
    pub show_master: bool,
    pub unlock_error: Option<store::Error>,
    pub reveal: bool,

    pub setup_password: String,
    pub setup_repeat: String,
    pub setup_error: Option<SetupError>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Tab(Tab),
    // Generator
    Length(u8),
    ToggleUpper(bool),
    ToggleLower(bool),
    ToggleDigits(bool),
    ToggleSymbols(bool),
    ToggleSimilar(bool),
    Generate,
    Copy,
    LabelChanged(String),
    Save,
    // Verlauf
    MasterInput(String),
    ToggleShowMaster,
    Unlock,
    Lock,
    ToggleReveal,
    CopyDraft(u64),
    DiscardDraft(u64),
    CopySaved(usize),
    // Ersteinrichtung
    SetupPassword(String),
    SetupRepeat(String),
    CreateVault,
    /// Entsperren oder Anlegen ist fertig (aus dem Hintergrund-Thread).
    VaultReady(Handoff),
    // Fenster und Zeit
    Tick(Instant),
    Minimized(Option<bool>),
    CloseRequested,
    SystemTheme(iced::theme::Mode),
    // Eigene Titelleiste
    DragWindow,
    MinimizeWindow,
    // Bewegung
    Hover(Id, bool),
    Frame(Instant),
    /// Mausrad: Pixel, um die das Scroll-Ziel wandert (positiv = nach unten).
    Wheel(f32),
    Scrolled(scrollable::Viewport),
}

/// Kennung der Scrollfläche, damit der Versatz von außen gesetzt werden kann.
const SCROLL_ID: &str = "main-scroll";

/// Startoptionen für Screenshots und Tests: `--light`, `--dark`, `--verlauf` (Tab Verlauf),
/// `--no-motion` (alle Übergänge aus, wie bei „Animationen reduzieren“) und
/// `--datei <pfad>` (andere Verlaufsdatei, z. B. zum Ausprobieren ohne die echte).
struct Launch {
    choice: ThemeChoice,
    tab: Tab,
    reduce_motion: bool,
    vault_path: PathBuf,
}

impl Launch {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let file = args
            .iter()
            .position(|a| a == "--datei")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Self {
            choice: if has("--dark") {
                ThemeChoice::Dark
            } else if has("--light") {
                ThemeChoice::Light
            } else {
                ThemeChoice::System
            },
            tab: if has("--verlauf") {
                Tab::Vault
            } else {
                Tab::Generator
            },
            reduce_motion: has("--no-motion"),
            vault_path: file.unwrap_or_else(store::default_path),
        }
    }
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let launch = Launch::from_args();
        let now = Instant::now();
        let mut app = App {
            icons: Icons::new(),
            choice: launch.choice,
            system_mode: iced::theme::Mode::None,
            tab: launch.tab,
            opts: Options::default(),
            label: String::new(),
            drafts: Drafts::default(),
            notice: None,
            wiped_notice: false,
            now,
            motion: Motion::new(launch.reduce_motion),
            // Gibt es die Datei schon, braucht es nur das Passwort. Sonst die Ersteinrichtung.
            vault: if launch.vault_path.exists() {
                VaultState::Locked
            } else {
                VaultState::NoFile
            },
            vault_path: launch.vault_path,
            store: None,
            busy: false,
            master_input: String::new(),
            show_master: false,
            unlock_error: None,
            reveal: false,
            setup_password: String::new(),
            setup_repeat: String::new(),
            setup_error: None,
        };
        // Anfangszustände ohne Übergang: nichts bewegt sich beim Start.
        let o = app.opts;
        for (i, on) in [o.upper, o.lower, o.digits, o.symbols, o.avoid_similar]
            .into_iter()
            .enumerate()
        {
            app.motion.init_on(Id::Toggle(i as u8), on);
        }
        app.motion.init_on(Id::TabVault, launch.tab == Tab::Vault);
        app.motion.init_strength((o.entropy_bits() / 128.0).min(1.0));
        app.generate(false);
        (app, system::theme().map(Message::SystemTheme))
    }

    /// Gespeicherte Einträge der entsperrten Datei, neueste zuerst. Leer, solange gesperrt.
    pub fn saved(&self) -> &[store::Entry] {
        self.store.as_ref().map_or(&[], Vault::entries)
    }

    /// Schlüssel und Einträge verwerfen (Sperren, Schließen).
    fn lock(&mut self) {
        self.store = None;
        self.vault = if self.vault_path.exists() {
            VaultState::Locked
        } else {
            VaultState::NoFile
        };
        self.reveal = false;
        self.master_input.zeroize();
    }

    pub fn tokens(&self) -> Tokens {
        match self.choice {
            ThemeChoice::Light => Tokens::light(),
            ThemeChoice::Dark => Tokens::dark(),
            ThemeChoice::System => match self.system_mode {
                iced::theme::Mode::Dark => Tokens::dark(),
                _ => Tokens::light(),
            },
        }
    }

    pub fn theme(&self) -> Theme {
        self.tokens().iced_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            // Der Takt prüft den 30-Sekunden-Timer und fragt ab, ob das Fenster minimiert ist.
            // iced meldet das Minimieren nicht als Ereignis.
            iced::time::every(Duration::from_millis(500)).map(Message::Tick),
            window::close_requests().map(|_| Message::CloseRequested),
            system::theme_changes().map(Message::SystemTheme),
        ];
        // Bildschritte nur, solange etwas animiert. Sonst ruht die Oberfläche.
        if self.motion.animating(Instant::now()) {
            subs.push(window::frames().map(Message::Frame));
        }
        Subscription::batch(subs)
    }

    /// Setzt einen Schalter und lässt seinen Knopf gleiten.
    fn set_toggle(&mut self, index: u8, on: bool) {
        self.motion.set(Key::On(Id::Toggle(index)), on, Instant::now());
        self.generate(true);
    }

    /// Erzeugt ein Passwort als Entwurf. Mit `replace_current` ersetzt es das aktuelle,
    /// noch nicht gespeicherte Passwort (beim Ändern der Einstellungen).
    fn generate(&mut self, replace_current: bool) {
        if replace_current {
            if let Some(d) = self.drafts.current() {
                if !d.saved {
                    let id = d.id;
                    self.drafts.discard(id);
                }
            }
        }
        if let Some(pw) = generator::generate(&self.opts, &mut rand::rng()) {
            self.drafts.push(pw, self.opts, Instant::now());
        }
        // Der Stärkebalken wächst oder schrumpft weich auf den neuen Wert.
        self.motion
            .set_strength((self.opts.entropy_bits() / 128.0).min(1.0), Instant::now());
        self.notice = None;
        self.wiped_notice = false;
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tab(tab) => {
                if tab != self.tab {
                    self.tab = tab;
                    // Die helle Fläche gleitet unter den neuen Tab, der Inhalt beginnt oben.
                    self.motion
                        .set(Key::On(Id::TabVault), tab == Tab::Vault, Instant::now());
                    self.motion.scroll.reset();
                    return scroll_to(
                        iced::widget::Id::new(SCROLL_ID),
                        AbsoluteOffset { x: None, y: Some(0.0) },
                    );
                }
            }

            Message::Length(v) => {
                self.opts.length = v;
                self.generate(true);
            }
            Message::ToggleUpper(v) => {
                self.opts.upper = v;
                self.set_toggle(0, v);
            }
            Message::ToggleLower(v) => {
                self.opts.lower = v;
                self.set_toggle(1, v);
            }
            Message::ToggleDigits(v) => {
                self.opts.digits = v;
                self.set_toggle(2, v);
            }
            Message::ToggleSymbols(v) => {
                self.opts.symbols = v;
                self.set_toggle(3, v);
            }
            Message::ToggleSimilar(v) => {
                self.opts.avoid_similar = v;
                self.set_toggle(4, v);
            }
            Message::Generate => self.generate(false),
            Message::Copy => {
                if let Some(d) = self.drafts.current() {
                    self.notice = Some(Notice::Copied);
                    return iced::clipboard::write(d.password.as_str().to_owned());
                }
            }
            Message::LabelChanged(s) => self.label = s,
            Message::Save => self.save_current(),

            // Bei jedem Tastendruck kommt ein neuer String. Der alte wird vorher überschrieben.
            Message::MasterInput(s) => {
                self.master_input.zeroize();
                self.master_input = s;
                self.unlock_error = None;
            }
            Message::ToggleShowMaster => self.show_master = !self.show_master,
            Message::Unlock => {
                if self.busy || self.vault != VaultState::Locked || self.master_input.is_empty() {
                    return Task::none();
                }
                self.busy = true;
                self.unlock_error = None;
                // Das Feld leert sich: Das Passwort wandert in den Thread und wird danach überschrieben.
                let password = Zeroizing::new(std::mem::take(&mut self.master_input));
                let path = self.vault_path.clone();
                return Task::perform(
                    in_background(move || Vault::open(&path, &password)),
                    Message::VaultReady,
                );
            }
            Message::Lock => self.lock(),
            Message::ToggleReveal => self.reveal = !self.reveal,
            Message::CopyDraft(id) => {
                if let Some(pw) = self.drafts.password(id) {
                    return iced::clipboard::write(pw.to_owned());
                }
            }
            Message::DiscardDraft(id) => self.drafts.discard(id),
            Message::CopySaved(i) => {
                if let Some(e) = self.saved().get(i) {
                    return iced::clipboard::write(e.password.as_str().to_owned());
                }
            }

            Message::SetupPassword(s) => {
                self.setup_password.zeroize();
                self.setup_password = s;
                self.setup_error = None;
            }
            Message::SetupRepeat(s) => {
                self.setup_repeat.zeroize();
                self.setup_repeat = s;
                self.setup_error = None;
            }
            Message::CreateVault => {
                if self.busy || self.vault != VaultState::NoFile {
                    return Task::none();
                }
                if self.setup_password.chars().count() < MIN_MASTER_LEN {
                    self.setup_error = Some(SetupError::TooShort);
                } else if self.setup_password != self.setup_repeat {
                    self.setup_error = Some(SetupError::Mismatch);
                } else {
                    self.busy = true;
                    let password = Zeroizing::new(std::mem::take(&mut self.setup_password));
                    self.setup_repeat.zeroize();
                    let path = self.vault_path.clone();
                    return Task::perform(
                        in_background(move || Vault::create(&path, &password, KdfParams::DEFAULT)),
                        Message::VaultReady,
                    );
                }
            }
            Message::VaultReady(handoff) => {
                self.busy = false;
                match handoff.take() {
                    Some(Ok(vault)) => {
                        self.store = Some(vault);
                        self.vault = VaultState::Unlocked;
                        self.show_master = false;
                    }
                    // Inzwischen gibt es eine Datei (z. B. aus einem zweiten Fenster): entsperren statt anlegen.
                    Some(Err(store::Error::Exists)) => self.vault = VaultState::Locked,
                    Some(Err(_)) if self.vault == VaultState::NoFile => {
                        self.setup_error = Some(SetupError::Failed);
                    }
                    Some(Err(e)) => self.unlock_error = Some(e),
                    None => {}
                }
            }

            Message::Tick(now) => {
                self.now = now;
                self.motion.refresh_reduced();
                if self.drafts.tick(now) {
                    self.wiped_notice = true;
                    self.notice = None;
                }
                return window::oldest()
                    .and_then(window::is_minimized)
                    .map(Message::Minimized);
            }
            Message::Minimized(state) => match state {
                Some(true) => self.drafts.window_hidden(Instant::now()),
                _ => self.drafts.window_visible(),
            },
            Message::CloseRequested => {
                // Beim Schließen sofort löschen, bevor der Prozess endet.
                self.drafts.wipe();
                self.lock();
                self.setup_password.zeroize();
                self.setup_repeat.zeroize();
                return iced::exit();
            }
            Message::SystemTheme(mode) => self.system_mode = mode,
            Message::DragWindow => return window::oldest().and_then(window::drag),
            Message::MinimizeWindow => {
                return window::oldest().and_then(|id| window::minimize(id, true));
            }

            Message::Hover(id, over) => {
                self.motion.set(Key::Hover(id), over, Instant::now());
            }
            // Ein Bildschritt: die Ansicht wird neu gebaut und liest die neuen Werte.
            // Beim weichen Scrollen setzt er außerdem den Versatz.
            Message::Frame(at) => {
                if self.motion.scroll.step(at) {
                    return self.apply_scroll();
                }
            }
            Message::Wheel(delta) => {
                let instant = self.motion.reduced();
                self.motion.scroll.wheel(delta, instant);
                if instant {
                    return self.apply_scroll();
                }
            }
            Message::Scrolled(viewport) => {
                let max = (viewport.content_bounds().height - viewport.bounds().height).max(0.0);
                self.motion.scroll.report(viewport.absolute_offset().y, max);
            }
        }
        Task::none()
    }

    fn apply_scroll(&self) -> Task<Message> {
        scroll_to(
            iced::widget::Id::new(SCROLL_ID),
            AbsoluteOffset {
                x: None,
                y: Some(self.motion.scroll.current),
            },
        )
    }

    fn save_current(&mut self) {
        let Some(store) = self.store.as_mut() else {
            return;
        };
        let Some(d) = self.drafts.current() else {
            return;
        };
        if d.saved {
            return;
        }
        let id = d.id;
        let entry = store::Entry {
            label: Zeroizing::new(self.label.trim().to_owned()),
            password: Zeroizing::new(d.password.as_str().to_owned()),
            saved_at: chrono::Utc::now().timestamp(),
            options: d.options,
        };
        // Verschlüsseln und Schreiben dauert nur Millisekunden (keine Schlüsselableitung).
        match store.add(entry) {
            Ok(()) => {
                self.drafts.mark_saved(id);
                self.label.clear();
                self.notice = Some(Notice::Saved);
            }
            Err(_) => self.notice = Some(Notice::SaveFailed),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        // Alle Bewegungswerte dieses Bildes werden zum selben Zeitpunkt gelesen.
        let fx = Fx::new(&self.motion, Instant::now());
        let fx = &fx;

        // Eigene Titelleiste: Titel und freie Fläche ziehen das Fenster, rechts Profil,
        // Minimieren und Schließen.
        let drag_area = iced::widget::mouse_area(
            row![
                widgets::txt("PassKey", 20.0, DISPLAY, t.ink),
                Space::new().width(Length::Fill),
            ]
            .height(44)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_press(Message::DragWindow);

        let header = row![
            drag_area,
            widgets::button_el(
                t,
                fx,
                Id::Profile,
                Variant::Secondary,
                Some(&self.icons.user),
                None,
                Some(Message::Tab(Tab::Vault)),
                Length::Fixed(44.0),
            ),
            widgets::button_el(
                t,
                fx,
                Id::Minimize,
                Variant::Ghost,
                Some(&self.icons.minus),
                None,
                Some(Message::MinimizeWindow),
                Length::Fixed(44.0),
            ),
            widgets::button_el(
                t,
                fx,
                Id::Close,
                Variant::Close,
                Some(&self.icons.x),
                None,
                Some(Message::CloseRequested),
                Length::Fixed(44.0),
            ),
        ]
        .spacing(theme::SPACE_1)
        .align_y(iced::alignment::Vertical::Center);

        let tabs = widgets::tabs_el(
            t,
            fx,
            ("Erzeugen", Id::TabGenerator, Message::Tab(Tab::Generator)),
            ("Verlauf", Id::TabVault, Message::Tab(Tab::Vault)),
            fx.on(Id::TabVault),
        );

        let body: Element<'_, Message> = match self.tab {
            Tab::Generator => crate::ui::generator::view(self, t, fx),
            Tab::Vault => match self.vault {
                VaultState::NoFile => crate::ui::setup::view(self, t, fx),
                VaultState::Locked => crate::ui::vault::locked(self, t, fx),
                VaultState::Unlocked => crate::ui::vault::unlocked(self, t, fx),
            },
        };

        // Dünne Scrollleiste in Kiesel-Farben, nur sichtbar, wenn der Inhalt länger ist.
        // Die Fläche reicht über die volle Fensterbreite. Der Inhalt hat links und rechts den
        // Seitenrand wie Kopfzeile und Tabs, die schmale Leiste sitzt im rechten Rand. So sind
        // alle Panels so breit wie die Tab-Leiste.
        let scroll = scrollable(container(body).padding(iced::Padding {
            left: theme::SPACE_4,
            right: theme::SPACE_4,
            ..iced::Padding::ZERO
        }))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(4).scroller_width(4).margin(2),
        ))
        .style(move |theme: &Theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail.background = None;
            style.vertical_rail.scroller.background = Background::Color(t.control_border);
            style
        })
        .id(iced::widget::Id::new(SCROLL_ID))
        .on_scroll(Message::Scrolled)
        .height(Length::Fill);

        // Weiches Scrollen: Eine durchsichtige Ebene über der Fläche fängt das Mausrad ab und
        // setzt nur das Ziel. Klicks und Hover gehen unverändert an die Elemente darunter.
        let wheel = mouse_area(Space::new().width(Length::Fill).height(Length::Fill)).on_scroll(
            |delta| {
                let px = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                Message::Wheel(px)
            },
        );
        let scroll = stack![scroll, wheel].height(Length::Fill);

        let side = iced::Padding {
            left: theme::SPACE_4,
            right: theme::SPACE_4,
            ..iced::Padding::ZERO
        };
        container(
            column![
                container(header).padding(side),
                container(tabs).padding(side),
                scroll
            ]
            .spacing(theme::SPACE_3),
        )
        .padding(iced::Padding {
            top: theme::SPACE_4,
            bottom: theme::SPACE_4,
            ..iced::Padding::ZERO
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(t.paper)),
            text_color: Some(t.ink),
            // Ohne Systemrahmen braucht das Fenster eine feine Kante.
            border: iced::Border {
                color: t.control_border,
                width: 1.0,
                radius: theme::RADIUS_LG.into(),
            },
            ..container::Style::default()
        })
        .into()
    }
}
