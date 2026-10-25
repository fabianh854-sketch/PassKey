//! Ansicht „Verlauf“: gesperrt (Master-Passwort) und entsperrt (Entwürfe + gespeicherte Einträge).

use std::time::Instant;

use chrono::{DateTime, Local, TimeZone};
use secrecy::ExposeSecret;
use iced::alignment::Vertical;
use iced::widget::{column, row, text, Space};
use iced::{Element, Length};

use super::widgets::*;
use crate::app::{App, Message};
use crate::motion::{Fx, Id};
use crate::store;
use crate::theme::*;

const MASK: &str = "••••••••••••";

pub fn locked<'a>(app: &'a App, t: Tokens, fx: &Fx) -> Element<'a, Message> {
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

    // Falsches Passwort und veränderte Datei sehen für die Entschlüsselung gleich aus.
    let error = app.unlock_error.map(|e| match e {
        store::Error::WrongPassword => "Das Passwort passt nicht. Versuch es noch einmal.",
        store::Error::Corrupt => "Die Datei ist beschädigt oder stammt aus einer neueren Version.",
        store::Error::Io(_) => "Die Datei lässt sich nicht lesen.",
        store::Error::Exists | store::Error::Invalid | store::Error::Internal => {
            "Das Entsperren ist fehlgeschlagen."
        }
    });
    let busy = app.busy;

    let card = card_el(
        t,
        column![
            row![
                txt("Verlauf entsperren", 20.0, DISPLAY, t.ink),
                Space::new().width(Length::Fill),
                badge_el(t, Tone::Neutral, "Gesperrt"),
            ]
            .align_y(Vertical::Center),
            txt(
                "Gib dein Master-Passwort ein, um den gespeicherten Verlauf zu sehen.",
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
                &app.master_input,
                !app.show_master,
                Message::MasterInput,
                (!busy).then_some(Message::Unlock),
                None,
                error,
                Some(eye),
            ),
            button_el(
                t,
                fx,
                Id::Unlock,
                Variant::Primary,
                Some(&icons.lock_open),
                Some(if busy { "Wird entsperrt …" } else { "Entsperren" }),
                (!busy && !app.master_input.is_empty()).then_some(Message::Unlock),
                Length::Fill,
            ),
        ]
        .spacing(SPACE_4),
    );

    column![card].into()
}

pub fn unlocked<'a>(app: &'a App, t: Tokens, fx: &Fx) -> Element<'a, Message> {
    let icons = &app.icons;

    let top = row![
        badge_el(t, Tone::Success, "Entsperrt"),
        Space::new().width(Length::Fill),
        button_el(
            t,
            fx,
            Id::Reveal,
            Variant::Ghost,
            Some(if app.reveal { &icons.eye_off } else { &icons.eye }),
            Some(if app.reveal { "Verbergen" } else { "Anzeigen" }),
            Some(Message::ToggleReveal),
            Length::Fixed(124.0),
        ),
        button_el(
            t,
            fx,
            Id::Lock,
            Variant::Outline,
            Some(&icons.lock),
            Some("Sperren"),
            Some(Message::Lock),
            Length::Fixed(116.0),
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Vertical::Center);

    // --- Nicht gespeichert (nur im RAM) ---
    let mut drafts_col = column![
        row![
            txt("Nicht gespeichert", 18.0, DISPLAY, t.ink),
            Space::new().width(Length::Fill),
            badge_el(t, Tone::Neutral, format!("{} offen", app.drafts.unsaved_count())),
        ]
        .align_y(Vertical::Center),
        txt(
            "Liegt nur im Arbeitsspeicher. Nach dem Minimieren löscht PassKey es nach 30 Sekunden.",
            13.0,
            INTER,
            t.ink_muted,
        ),
    ]
    .spacing(SPACE_2);

    if app.drafts.unsaved_count() == 0 {
        drafts_col = drafts_col.push(txt("Keine offenen Passwörter.", 15.0, INTER, t.ink_muted));
    }
    for d in app.drafts.unsaved() {
        let shown: &str = if app.reveal { d.password.expose_secret() } else { MASK };
        drafts_col = drafts_col.push(divider_el(t)).push(list_row(
            t,
            shown,
            age(app.now, d.created),
            vec![
                button_el(
                    t,
                    fx,
                    Id::CopyDraft(d.id),
                    Variant::Secondary,
                    Some(&icons.copy),
                    None,
                    Some(Message::CopyDraft(d.id)),
                    Length::Fixed(44.0),
                ),
                button_el(
                    t,
                    fx,
                    Id::DiscardDraft(d.id),
                    Variant::Secondary,
                    Some(&icons.x),
                    None,
                    Some(Message::DiscardDraft(d.id)),
                    Length::Fixed(44.0),
                ),
            ],
            app.reveal,
        ));
    }

    // --- Gespeichert ---
    let mut saved_col = column![
        row![
            txt("Gespeichert", 18.0, DISPLAY, t.ink),
            Space::new().width(Length::Fill),
            badge_el(t, Tone::Neutral, format!("{}", app.saved().len())),
        ]
        .align_y(Vertical::Center),
    ]
    .spacing(SPACE_2);

    let today = Local::now();
    if app.saved().is_empty() {
        saved_col = saved_col.push(txt(
            "Noch nichts gespeichert. Speichere ein Passwort im Tab Erzeugen.",
            15.0,
            INTER,
            t.ink_muted,
        ));
    }
    for (i, e) in app.saved().iter().enumerate() {
        let title = if e.label().is_empty() { "Ohne Etikett" } else { e.label() };
        let pw: &str = if app.reveal { e.password() } else { MASK };
        saved_col = saved_col.push(divider_el(t)).push(list_row_titled(
            t,
            title,
            pw,
            when(e.saved_at(), today),
            vec![button_el(
                t,
                fx,
                Id::CopySaved(i),
                Variant::Secondary,
                Some(&icons.copy),
                None,
                Some(Message::CopySaved(i)),
                Length::Fixed(44.0),
            )],
        ));
    }

    column![top, card_el(t, drafts_col), card_el(t, saved_col)]
        .spacing(SPACE_4)
        .into()
}

/// Zeile für einen Entwurf: Passwort als Titel, Alter als Nebentext.
fn list_row<'a>(
    t: Tokens,
    title: &'a str,
    meta: String,
    trailing: Vec<Element<'a, Message>>,
    wrap_glyphs: bool,
) -> Element<'a, Message> {
    let mut title_text = txt(title, 15.0, INTER_MEDIUM, t.ink);
    if wrap_glyphs {
        title_text = title_text.wrapping(text::Wrapping::Glyph);
    }
    let main = column![title_text, txt(meta, 13.0, INTER, t.ink_muted)]
        .spacing(2)
        .width(Length::Fill);
    row_with(main, trailing)
}

/// Zeile für einen gespeicherten Eintrag: Etikett, Passwort und Datum.
fn list_row_titled<'a>(
    t: Tokens,
    title: &'a str,
    password: &'a str,
    when: String,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let main = column![
        // Etiketten bis 64 Zeichen: an Wörtern umbrechen, ein langes Wort notfalls mittendrin.
        txt(title, 15.0, INTER_MEDIUM, t.ink).wrapping(text::Wrapping::WordOrGlyph),
        txt(password, 14.0, INTER, t.ink).wrapping(text::Wrapping::Glyph),
        txt(when, 13.0, INTER, t.ink_muted),
    ]
    .spacing(2)
    .width(Length::Fill);
    row_with(main, trailing)
}

fn row_with<'a>(
    main: impl Into<Element<'a, Message>>,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut r = row![main.into()].spacing(SPACE_2).align_y(Vertical::Center);
    for el in trailing {
        r = r.push(el);
    }
    iced::widget::container(r).padding([SPACE_2, 0.0]).into()
}

/// Zeitpunkt eines gespeicherten Eintrags in lokaler Zeit, gemessen an `today` (jetzt).
///
/// Das Design aus Phase 1 zeigte: „Heute, 09:41“, „Gestern, 18:05“, „12. Okt., 21:30“.
fn when(saved_at: i64, today: DateTime<Local>) -> String {
    let Some(at) = Local.timestamp_opt(saved_at, 0).single() else {
        return "Unbekannte Zeit".to_owned();
    };
    let _ = today;
    // TODO(Fabian): freundliche Form statt der vollen Zahl (siehe Doku oben). Nützlich:
    // `at.date_naive()`, `today.date_naive().pred_opt()`, `at.month()`, `at.year()` (Trait `Datelike`).
    at.format("%d.%m.%Y, %H:%M").to_string()
}

/// „Gerade eben“, „Vor 20 Sek.“, „Vor 3 Min.“
fn age(now: Instant, created: Instant) -> String {
    let secs = now.saturating_duration_since(created).as_secs();
    match secs {
        0..=9 => "Gerade eben".to_owned(),
        10..=59 => format!("Vor {secs} Sek."),
        _ => format!("Vor {} Min.", secs / 60),
    }
}
