#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod drafts;
mod generator;
mod motion;
mod store;
mod theme;
mod ui;

use app::App;

fn main() -> iced::Result {
    let mut application = iced::application(App::boot, App::update, App::view)
        .title("PassKey")
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(theme::INTER)
        .window_size((theme::WINDOW_W, 700.0))
        .resizable(false)
        // Die Titelleiste ist Teil des Hauptfensters (siehe `App::view`).
        .decorations(false)
        // Durchsichtiges Fenster, damit die runden Ecken der Oberfläche sichtbar sind.
        .transparent(true)
        .style(|_, theme| iced::theme::Style {
            background_color: iced::Color::TRANSPARENT,
            text_color: theme.palette().text,
        })
        .centered()
        .antialiasing(true)
        // Das Schließen läuft über `App::update`, damit die Entwürfe vorher gelöscht werden.
        .exit_on_close_request(false);

    for font in theme::FONT_FILES {
        application = application.font(font);
    }
    application.run()
}
