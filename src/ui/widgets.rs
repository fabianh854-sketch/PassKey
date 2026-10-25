//! Kiesel-Komponenten für iced: Button, Field, Toggle, Slider, Tabs, Badge, Alert, Card,
//! Progress, ListRow-Trenner. Alle Farben, Radien und Maße kommen aus `theme`.
//! Bewegung (Hover, Schalter, Tabs, Slider) kommt aus `motion` über `Fx`.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    button, column, container, mouse_area, progress_bar, row, slider, stack, svg, text,
    text_input, Space,
};
use iced::{Background, Border, Color, Element, Font, Length, Theme};

use crate::app::Message;
use crate::motion::{Fx, Id};
use crate::theme::*;

// --- Icons -------------------------------------------------------------------

/// Die eingebetteten Lucide-Icons (Linienstil, Strichstärke 1,8). Einmal erzeugt, dann geklont.
pub struct Icons {
    pub user: svg::Handle,
    pub copy: svg::Handle,
    pub lock: svg::Handle,
    pub lock_open: svg::Handle,
    pub eye: svg::Handle,
    pub eye_off: svg::Handle,
    pub refresh: svg::Handle,
    pub x: svg::Handle,
    pub save: svg::Handle,
    pub shield: svg::Handle,
    pub info: svg::Handle,
    pub alert: svg::Handle,
    pub minus: svg::Handle,
}

impl Icons {
    pub fn new() -> Self {
        let h = |bytes: &'static [u8]| svg::Handle::from_memory(bytes);
        Self {
            user: h(include_bytes!("../../assets/icons/user.svg")),
            copy: h(include_bytes!("../../assets/icons/copy.svg")),
            lock: h(include_bytes!("../../assets/icons/lock.svg")),
            lock_open: h(include_bytes!("../../assets/icons/lock-open.svg")),
            eye: h(include_bytes!("../../assets/icons/eye.svg")),
            eye_off: h(include_bytes!("../../assets/icons/eye-off.svg")),
            refresh: h(include_bytes!("../../assets/icons/refresh-cw.svg")),
            x: h(include_bytes!("../../assets/icons/x.svg")),
            save: h(include_bytes!("../../assets/icons/save.svg")),
            shield: h(include_bytes!("../../assets/icons/shield-check.svg")),
            info: h(include_bytes!("../../assets/icons/info.svg")),
            alert: h(include_bytes!("../../assets/icons/circle-alert.svg")),
            minus: h(include_bytes!("../../assets/icons/minus.svg")),
        }
    }
}

pub fn icon<'a, M: 'a>(handle: &svg::Handle, size: f32, color: Color) -> Element<'a, M> {
    svg(handle.clone())
        .width(size)
        .height(size)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

// --- Text --------------------------------------------------------------------

pub fn txt<'a>(
    s: impl text::IntoFragment<'a>,
    size: f32,
    font: Font,
    color: Color,
) -> text::Text<'a> {
    text(s).size(size).font(font).color(color)
}

// --- Hover -------------------------------------------------------------------

/// Meldet, wann der Mauszeiger ein Element betritt oder verlässt. Daraus entsteht der Übergang.
fn hoverable<'a>(id: Id, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content)
        .on_enter(Message::Hover(id, true))
        .on_exit(Message::Hover(id, false))
        .into()
}

// --- Button ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Outline,
    Ghost,
    /// Schließen-Knopf der Titelleiste: wie `Ghost`, beim Hover in `clay-mark` (Kiesels Fehlerton).
    Close,
}

/// Kiesel-Button, 44 px hoch. `on_press = None` ergibt den deaktivierten Zustand.
/// Hover legt eine Überlagerung in Textfarbe ein (bis 8 %), Druck eine kräftigere (14 %).
#[allow(clippy::too_many_arguments)]
pub fn button_el<'a>(
    t: Tokens,
    fx: &Fx,
    id: Id,
    variant: Variant,
    icon_handle: Option<&svg::Handle>,
    label: Option<&'a str>,
    on_press: Option<Message>,
    width: Length,
) -> Element<'a, Message> {
    let (bg, fg, border) = match variant {
        Variant::Primary => (Some(t.accent), t.on_accent, None),
        Variant::Secondary => (Some(t.fog), t.ink, None),
        Variant::Outline => (None, t.ink, Some(t.control_border)),
        Variant::Ghost | Variant::Close => (None, t.accent_text, None),
    };
    let enabled = on_press.is_some();
    let h = fx.hover(id);
    // Beim Schließen-Knopf wechselt das Icon beim Hover mit der Fläche nach on-accent.
    let fg = if variant == Variant::Close { mix(fg, t.on_accent, h) } else { fg };
    let content_color = if enabled { fg } else { alpha(fg, 0.45) };

    let mut content = row![].spacing(SPACE_2).align_y(Vertical::Center);
    if let Some(handle) = icon_handle {
        content = content.push(icon(handle, 18.0, content_color));
    }
    if let Some(label) = label {
        content = content.push(txt(label, 15.0, INTER_MEDIUM, content_color));
    }

    let inner = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center);

    let btn = button(inner)
        .width(width)
        .height(44)
        .padding([0, 12])
        .on_press_maybe(on_press)
        .style(move |_: &Theme, status| {
            let over = |amount: f32| match bg {
                Some(b) => Some(mix(b, fg, amount)),
                None => Some(alpha(fg, amount)),
            };
            if variant == Variant::Close {
                // Rote Fläche blendet beim Hover ein, beim Drücken wird sie etwas dunkler.
                let background = match status {
                    button::Status::Pressed => mix(t.clay_mark, Color::BLACK, 0.12),
                    _ => alpha(t.clay_mark, h),
                };
                return button::Style {
                    background: Some(Background::Color(background)),
                    text_color: fg,
                    border: Border {
                        radius: RADIUS_MD.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                };
            }
            // Outline-Buttons färben den Rand beim Hover von control-border nach ink-muted.
            let hover_border = border.map(|_| mix(t.control_border, t.ink_muted, h));
            let (background, border_color) = match status {
                button::Status::Active | button::Status::Hovered => (over(0.08 * h), hover_border),
                button::Status::Pressed => (over(0.14), hover_border),
                button::Status::Disabled => {
                    (bg.map(|b| alpha(b, 0.45)), border.map(|b| alpha(b, 0.45)))
                }
            };
            button::Style {
                background: background.map(Background::Color),
                text_color: if enabled { fg } else { alpha(fg, 0.45) },
                border: Border {
                    color: border_color.unwrap_or(Color::TRANSPARENT),
                    width: if border.is_some() { 1.0 } else { 0.0 },
                    radius: RADIUS_MD.into(),
                },
                ..button::Style::default()
            }
        });
    hoverable(id, btn)
}

// --- Field -------------------------------------------------------------------

/// Eingabefeld mit sichtbarer Beschriftung, Hilfetext und Fehlermeldung.
/// Der Rand wechselt beim Hover weich von control-border nach ink-muted, im Fokus nach accent.
#[allow(clippy::too_many_arguments)]
pub fn field_el<'a>(
    t: Tokens,
    fx: &Fx,
    id: Id,
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    secure: bool,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Option<Message>,
    hint: Option<&'a str>,
    error: Option<&'a str>,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let has_error = error.is_some();
    let h = fx.hover(id);
    let mut input = text_input(placeholder, value)
        .on_input(on_input)
        .secure(secure)
        .size(15)
        .font(INTER)
        .padding([12, 14])
        .style(move |_: &Theme, status| {
            let (width, color) = match status {
                text_input::Status::Active | text_input::Status::Hovered => {
                    (1.0, mix(t.control_border, t.ink_muted, h))
                }
                text_input::Status::Focused { .. } => (2.0, t.accent),
                text_input::Status::Disabled => (1.0, t.line),
            };
            let color = if has_error { t.clay_mark } else { color };
            text_input::Style {
                background: Background::Color(t.surface),
                border: Border {
                    color,
                    width,
                    radius: RADIUS_MD.into(),
                },
                icon: t.ink_muted,
                placeholder: t.ink_muted,
                value: t.ink,
                selection: alpha(t.accent, 0.3),
            }
        });
    if let Some(msg) = on_submit {
        input = input.on_submit(msg);
    }

    let mut line = row![hoverable(id, input)]
        .spacing(SPACE_2)
        .align_y(Vertical::Center);
    if let Some(extra) = trailing {
        line = line.push(extra);
    }

    let mut col = column![txt(label, 14.0, INTER_MEDIUM, t.ink), line].spacing(6);
    if let Some(e) = error {
        // Fehler stehen als Text, nie nur als Farbe.
        col = col.push(txt(e, 13.0, INTER, t.clay_mark));
    } else if let Some(h) = hint {
        col = col.push(txt(h, 13.0, INTER, t.ink_muted));
    }
    col.into()
}

// --- Toggle ------------------------------------------------------------------

const TRACK_W: f32 = 44.0;
const TRACK_H: f32 = 26.0;
const KNOB: f32 = 20.0;
const TRACK_PAD: f32 = 3.0;

/// Schalterzeile: Beschriftung links, Schalter rechts, mindestens 44 px hoch.
/// Der Knopf gleitet in `duration-base`, die Spur wechselt die Farbe weich.
pub fn toggle_el<'a>(
    t: Tokens,
    fx: &Fx,
    id: Id,
    label: &'a str,
    value: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    let k = fx.on(id);
    let h = fx.hover(id);

    // An: accent mit Knopf in on-accent. Aus: control-border mit Knopf in surface.
    let track_color = mix(mix(t.control_border, t.accent, k), Color::BLACK, 0.06 * h);
    let knob_color = mix(t.surface, t.on_accent, k);
    let travel = TRACK_W - 2.0 * TRACK_PAD - KNOB;

    let knob = container(Space::new().width(KNOB).height(KNOB)).style(move |_: &Theme| {
        container::Style {
            background: Some(Background::Color(knob_color)),
            border: Border {
                radius: (KNOB / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    });
    let track = container(row![Space::new().width(Length::Fixed(travel * k)), knob])
        .padding(TRACK_PAD)
        .width(TRACK_W)
        .height(TRACK_H)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(track_color)),
            border: Border {
                radius: (TRACK_H / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    // Die ganze Zeile ist klickbar, auch die Beschriftung.
    mouse_area(
        row![txt(label, 14.0, INTER, t.ink).width(Length::Fill), track]
            .spacing(SPACE_3)
            .align_y(Vertical::Center)
            .height(44),
    )
    .on_press(on_toggle(!value))
    .on_enter(Message::Hover(id, true))
    .on_exit(Message::Hover(id, false))
    .into()
}

// --- Slider ------------------------------------------------------------------

/// Der Knopf wächst beim Hover leicht (1,08) und zieht sich beim Ziehen zusammen.
pub fn slider_el<'a>(
    t: Tokens,
    fx: &Fx,
    label: &'a str,
    value_text: String,
    range: std::ops::RangeInclusive<u8>,
    value: u8,
    on_change: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message> {
    let h = fx.hover(Id::Slider);
    let bar = slider(range, value, on_change)
        .height(28)
        .style(move |_: &Theme, status| {
            let radius = match status {
                slider::Status::Active | slider::Status::Hovered => 10.0 * (1.0 + 0.08 * h),
                slider::Status::Dragged => 9.6,
            };
            slider::Style {
                rail: slider::Rail {
                    backgrounds: (Background::Color(t.accent), Background::Color(t.fog)),
                    width: 6.0,
                    border: Border {
                        color: t.line,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                },
                handle: slider::Handle {
                    shape: slider::HandleShape::Circle { radius },
                    background: Background::Color(t.accent),
                    border_width: 3.0,
                    border_color: t.surface,
                },
            }
        });

    column![
        row![
            txt(label, 14.0, INTER_MEDIUM, t.ink),
            Space::new().width(Length::Fill),
            txt(value_text, 14.0, INTER, t.ink_muted),
        ]
        .align_y(Vertical::Center),
        hoverable(Id::Slider, bar)
    ]
    .spacing(SPACE_1)
    .into()
}

// --- Tabs --------------------------------------------------------------------

/// Breite eines Tabs: Fensterbreite minus Seitenrand, Leistenrand und Lücke, geteilt durch zwei.
const TAB_W: f32 = (WINDOW_W - 2.0 * SPACE_4 - 2.0 * SPACE_1 - 2.0) / 2.0;
const TAB_H: f32 = 36.0;

/// Zwei Tabs. Die helle Fläche des aktiven Tabs gleitet beim Wechsel unter den neuen Tab.
/// `thumb` ist die Position der Fläche von 0 (links) bis 1 (rechts).
pub fn tabs_el<'a>(
    t: Tokens,
    fx: &Fx,
    left: (&'a str, Id, Message),
    right: (&'a str, Id, Message),
    thumb: f32,
) -> Element<'a, Message> {
    let thumb_layer = row![
        Space::new().width(Length::Fixed(thumb * (TAB_W + 2.0))),
        container(Space::new())
            .width(TAB_W)
            .height(TAB_H)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border {
                    color: t.line,
                    width: 1.0,
                    radius: 9.0.into(),
                },
                ..container::Style::default()
            }),
    ]
    .width(Length::Fill)
    .height(TAB_H);

    let tab = |(label, id, msg): (&'a str, Id, Message), selected: f32| {
        // Inaktive Tabs färben sich beim Hover von ink-muted nach ink.
        let tone = selected.max(fx.hover(id));
        let color = mix(t.ink_muted, t.ink, tone);
        let btn = button(
            container(txt(label, 14.0, INTER_MEDIUM, color))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center),
        )
        .width(Length::Fill)
        .height(TAB_H)
        .padding(0)
        .on_press(msg)
        .style(move |_: &Theme, _| button::Style {
            background: None,
            text_color: color,
            border: Border::default(),
            ..button::Style::default()
        });
        hoverable(id, btn)
    };

    let buttons = row![tab(left, 1.0 - thumb), tab(right, thumb)]
        .spacing(2)
        .width(Length::Fill);

    container(stack![thumb_layer, buttons])
        .padding(SPACE_1)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(t.fog)),
            border: Border {
                radius: 12.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

// --- Badge -------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum Tone {
    Neutral,
    Success,
    Danger,
}

/// Zustand in einem Wort, mit Punkt und Farbfläche.
pub fn badge_el<'a, M: 'a>(t: Tokens, tone: Tone, label: impl text::IntoFragment<'a>) -> Element<'a, M> {
    let (bg, fg, dot) = match tone {
        Tone::Neutral => (t.fog, t.ink, t.ink),
        Tone::Success => (t.sage, t.on_sage, t.moss),
        Tone::Danger => (t.clay, t.on_clay, t.clay_mark),
    };
    let dot = container(Space::new().width(6).height(6)).style(move |_: &Theme| container::Style {
        background: Some(Background::Color(dot)),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });
    container(
        row![dot, txt(label, 13.0, INTER_MEDIUM, fg)]
            .spacing(6)
            .align_y(Vertical::Center),
    )
    .padding([6, 10])
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

// --- Alert -------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum AlertTone {
    Info,
    Danger,
}

/// Hinweis ohne Rand und ohne Farbbalken: Fläche und Icon genügen.
pub fn alert_el<'a, M: 'a>(
    t: Tokens,
    icons: &Icons,
    tone: AlertTone,
    title: Option<&'a str>,
    body: &'a str,
) -> Element<'a, M> {
    let (bg, fg, handle) = match tone {
        AlertTone::Info => (t.fog, t.ink, &icons.info),
        AlertTone::Danger => (t.clay, t.on_clay, &icons.alert),
    };
    let mut text_col = column![].spacing(2);
    if let Some(title) = title {
        text_col = text_col.push(txt(title, 14.0, INTER_SEMIBOLD, fg));
    }
    text_col = text_col.push(txt(body, 14.0, INTER, fg));

    container(
        row![icon(handle, 20.0, fg), text_col.width(Length::Fill)]
            .spacing(SPACE_3)
            .align_y(Vertical::Top),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: 12.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

// --- Card, Progress, Trenner -------------------------------------------------

pub fn card_el<'a, M: 'a>(t: Tokens, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(SPACE_5)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(t.surface)),
            text_color: Some(t.ink),
            border: Border {
                color: t.line,
                width: 1.0,
                radius: RADIUS_LG.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// Ruhige Füllung (`fog`) für ein Feld in einer Karte.
pub fn inset_el<'a, M: 'a>(t: Tokens, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding([SPACE_3, SPACE_4])
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(t.fog)),
            text_color: Some(t.ink),
            border: Border {
                radius: RADIUS_MD.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

pub fn progress_el<'a, M: 'a>(t: Tokens, value: f32) -> Element<'a, M> {
    progress_bar(0.0..=1.0, value)
        .girth(8)
        .style(move |_: &Theme| progress_bar::Style {
            background: Background::Color(t.fog),
            bar: Background::Color(t.accent),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
        })
        .into()
}

/// Haarlinie zwischen Listenzeilen (`line`).
pub fn divider_el<'a, M: 'a>(t: Tokens) -> Element<'a, M> {
    container(Space::new().width(Length::Fill).height(1))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(t.line)),
            ..container::Style::default()
        })
        .into()
}
