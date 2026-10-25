//! Ansicht „Erzeugen“: das Startfenster. Kein Passwort nötig.

use iced::alignment::Vertical;
use iced::widget::{column, container, row, text};
use iced::{Element, Length};

use super::widgets::*;
use crate::app::{App, Message, Notice, VaultState};
use crate::generator::{Strength, MAX_LEN, MIN_LEN};
use crate::motion::{Fx, Id};
use crate::theme::*;

pub fn view<'a>(app: &'a App, t: Tokens, fx: &Fx) -> Element<'a, Message> {
    let icons = &app.icons;
    let opts = &app.opts;
    let current = app.drafts.current();

    // Ausgabe: das aktuelle Passwort auf ruhiger Fläche.
    let output: Element<'a, Message> = match current {
        Some(d) => {
            let s = d.password.as_str();
            let size = match s.chars().count() {
                0..=14 => 20.0,
                15..=20 => 17.0,
                21..=26 => 15.0,
                _ => 14.0,
            };
            txt(s, size, INTER_MEDIUM, t.ink)
                .wrapping(text::Wrapping::Glyph)
                .into()
        }
        None => txt("Noch kein Passwort", 16.0, INTER, t.ink_muted).into(),
    };
    let output = inset_el(
        t,
        container(output)
            .width(Length::Fill)
            .height(44)
            .align_y(Vertical::Center),
    );

    // Stärke: Badge mit Wort, Balken (wächst weich) und Bit-Zahl.
    let bits = opts.entropy_bits();
    let strength = Strength::from_bits(bits);
    let tone = match strength {
        Strength::Weak => Tone::Danger,
        Strength::Fair => Tone::Neutral,
        Strength::Strong | Strength::VeryStrong => Tone::Success,
    };
    let strength_row = row![
        badge_el(t, tone, strength.label()),
        progress_el(t, fx.strength()),
        txt(format!("{bits:.0} Bit"), 13.0, INTER, t.ink_muted),
    ]
    .spacing(SPACE_3)
    .align_y(Vertical::Center);

    let length = slider_el(
        t,
        fx,
        "Länge",
        format!("{} Zeichen", opts.length()),
        MIN_LEN..=MAX_LEN,
        opts.length.clamp(MIN_LEN, MAX_LEN),
        Message::Length,
    );

    let toggles = column![
        row![
            toggle_el(t, fx, Id::Toggle(0), "Groß (A–Z)", opts.upper, Message::ToggleUpper),
            toggle_el(t, fx, Id::Toggle(1), "Klein (a–z)", opts.lower, Message::ToggleLower),
        ]
        .spacing(SPACE_5),
        row![
            toggle_el(t, fx, Id::Toggle(2), "Zahlen", opts.digits, Message::ToggleDigits),
            toggle_el(t, fx, Id::Toggle(3), "Symbole", opts.symbols, Message::ToggleSymbols),
        ]
        .spacing(SPACE_5),
        toggle_el(
            t,
            fx,
            Id::Toggle(4),
            "Ähnliche Zeichen meiden",
            opts.avoid_similar,
            Message::ToggleSimilar,
        ),
    ];

    let card = card_el(
        t,
        column![output, strength_row, length, toggles].spacing(SPACE_4),
    );

    // Aktionen: eine Primäraktion, daneben Kopieren.
    let actions = row![
        button_el(
            t,
            fx,
            Id::Generate,
            Variant::Primary,
            Some(&icons.refresh),
            Some("Neu erzeugen"),
            opts.has_class().then_some(Message::Generate),
            Length::FillPortion(3),
        ),
        button_el(
            t,
            fx,
            Id::Copy,
            Variant::Secondary,
            Some(&icons.copy),
            Some(if app.notice == Some(Notice::Copied) { "Kopiert" } else { "Kopieren" }),
            current.map(|_| Message::Copy),
            Length::FillPortion(2),
        ),
    ]
    .spacing(SPACE_2);

    // Speichern: nur bei entsperrter Datei und nur für ein noch nicht gespeichertes Passwort.
    let can_save = app.vault == VaultState::Unlocked && current.is_some_and(|d| !d.saved);
    let save_row = row![
        container(field_el(
            t,
            fx,
            Id::Field(2),
            "Etikett (optional)",
            "z. B. Bank",
            &app.label,
            false,
            Message::LabelChanged,
            can_save.then_some(Message::Save),
            None,
            None,
            None,
        ))
        .width(Length::Fill),
        button_el(
            t,
            fx,
            Id::Save,
            Variant::Outline,
            Some(&icons.save),
            Some("Speichern"),
            can_save.then_some(Message::Save),
            Length::Fixed(136.0),
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Vertical::Bottom);

    let status_text: &'static str = match (app.notice, app.vault) {
        (Some(Notice::Copied), _) => "In die Zwischenablage kopiert.",
        (Some(Notice::Saved), _) => "Im Verlauf gespeichert.",
        (Some(Notice::SaveFailed), _) => "Nicht gespeichert. Das Passwort bleibt als Entwurf offen.",
        (_, VaultState::NoFile) => "Zum Speichern erst den Verlauf anlegen.",
        (_, VaultState::Locked) => "Zum Speichern erst den Verlauf entsperren.",
        (_, VaultState::Unlocked) => "Offene Passwörter werden 30 s nach dem Minimieren gelöscht.",
    };

    let mut col = column![].spacing(SPACE_3);
    if app.wiped_notice {
        col = col.push(alert_el(
            t,
            icons,
            AlertTone::Info,
            Some("Gelöscht."),
            "Nicht gespeicherte Passwörter wurden aus dem Arbeitsspeicher entfernt.",
        ));
    }
    if app.notice == Some(Notice::SaveFailed) {
        col = col.push(alert_el(
            t,
            icons,
            AlertTone::Danger,
            Some("Speichern fehlgeschlagen."),
            "Die Verlaufsdatei ließ sich nicht schreiben. Der alte Stand ist unverändert.",
        ));
    }
    if !opts.has_class() {
        col = col.push(alert_el(
            t,
            icons,
            AlertTone::Danger,
            None,
            "Wähle mindestens eine Zeichenart, damit PassKey ein Passwort erzeugen kann.",
        ));
    }
    col.push(card)
        .push(actions)
        .push(save_row)
        .push(txt(status_text, 13.0, INTER, t.ink_muted))
        .into()
}
