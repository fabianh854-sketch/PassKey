//! Ansicht „Verlauf anlegen“: Ersteinrichtung, solange es noch keine Datei gibt.

use iced::alignment::Vertical;
use iced::widget::{column, row, Space};
use iced::{Element, Length};

use super::widgets::*;
use crate::app::{App, Message, SetupError, MIN_MASTER_LEN};
use crate::motion::{Fx, Id};
use crate::theme::*;

pub fn view<'a>(app: &'a App, t: Tokens, fx: &Fx) -> Element<'a, Message> {
    let icons = &app.icons;

    let eye = button_el(
        t,
        fx,
        Id::Eye,
        Variant::Outline,
        Some(if app.show_master { &icons.eye_off } else { &icons.eye }),
        None,
        Some(Message::ToggleShowMaster),
        Length::Fixed(44.0),
    );

    let (pw_error, repeat_error) = match app.setup_error {
        Some(SetupError::TooShort) => (
            Some("Das Passwort braucht mindestens 12 Zeichen. Wähle ein längeres."),
            None,
        ),
        Some(SetupError::Mismatch) => (None, Some("Die Passwörter stimmen nicht überein.")),
        Some(SetupError::Failed) => (
            None,
            Some("Die Datei ließ sich nicht anlegen. Prüf, ob der Ordner beschreibbar ist."),
        ),
        None => (None, None),
    };
    let busy = app.busy;
    debug_assert_eq!(MIN_MASTER_LEN, 12);

    let card = card_el(
        t,
        column![
            row![
                txt("Verlauf anlegen", 20.0, DISPLAY, t.ink),
                Space::new().width(Length::Fill),
                badge_el(t, Tone::Neutral, "Neu"),
            ]
            .align_y(Vertical::Center),
            txt(
                "Der Verlauf liegt verschlüsselt in einer Datei auf diesem Gerät. Lege ein Master-Passwort dafür fest.",
                15.0,
                INTER,
                t.ink_muted,
            ),
            field_el(
                t,
                fx,
                Id::Field(0),
                "Master-Passwort",
                "",
                &app.setup_password,
                !app.show_master,
                Message::SetupPassword,
                None,
                Some("Mindestens 12 Zeichen."),
                pw_error,
                Some(eye),
            ),
            field_el(
                t,
                fx,
                Id::Field(1),
                "Passwort wiederholen",
                "",
                &app.setup_repeat,
                !app.show_master,
                Message::SetupRepeat,
                (!busy).then_some(Message::CreateVault),
                None,
                repeat_error,
                None,
            ),
            alert_el(
                t,
                icons,
                AlertTone::Info,
                Some("Hinweis."),
                "Ohne dieses Passwort gibt es keinen Zugriff mehr. Es gibt keine Wiederherstellung.",
            ),
            button_el(
                t,
                fx,
                Id::CreateVault,
                Variant::Primary,
                Some(&icons.shield),
                Some(if busy { "Wird angelegt …" } else { "Verlauf anlegen" }),
                (!busy).then_some(Message::CreateVault),
                Length::Fill,
            ),
        ]
        .spacing(SPACE_4),
    );

    column![card].into()
}
