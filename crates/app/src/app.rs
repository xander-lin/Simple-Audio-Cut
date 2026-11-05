//! Entry point: window configuration, subscriptions and keyboard shortcuts.

use std::time::Duration;

use iced::window;
use iced::{keyboard, Size, Subscription, Task};

use crate::message::{Message, Shortcut};
use crate::services::Services;
use crate::theme;
use crate::update::App;

/// Launches an application whose initial state is produced by `boot`.
///
/// Used by `examples/seed_preview` for visual inspection of the editor with
/// mock services and synthetic tracks.
pub fn preview(
    boot: impl Fn() -> App + Clone + Send + Sync + 'static,
) -> iced::Application<impl iced::Program<State = App, Message = Message, Theme = iced::Theme>> {
    iced::application(boot, App::update, App::view)
        .title("Simple Audio Cut — preview")
        .theme(|_: &App| theme::theme())
        .window(window::Settings {
            size: Size::new(1100.0, 760.0),
            min_size: Some(Size::new(760.0, 600.0)),
            ..window::Settings::default()
        })
        .exit_on_close_request(false)
        .subscription(subscription)
}

pub fn run() -> iced::Result {
    #[cfg(feature = "native-audio")]
    let services = match Services::production() {
        Ok(services) => services,
        Err(error) => {
            eprintln!("Unable to initialise the native audio engine: {error}");
            eprintln!("Starting with headless audio services; recording and export are unavailable.");
            Services::mock()
        }
    };
    #[cfg(not(feature = "native-audio"))]
    let services = {
        eprintln!("Built without native audio: recording, playback and export use mock services.");
        Services::mock()
    };

    iced::application(
        move || App::boot(services.clone()),
        App::update,
        App::view,
    )
    .title("Simple Audio Cut")
    .theme(|_: &App| theme::theme())
    .window(window::Settings {
        size: Size::new(1100.0, 760.0),
        min_size: Some(Size::new(760.0, 600.0)),
        ..window::Settings::default()
    })
    .exit_on_close_request(false)
    .subscription(subscription)
    .run()
}

fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::batch([
        iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick),
        iced::event::listen_with(keyboard_shortcut),
        window::close_requests().map(|_| Message::CloseRequested),
        window::resize_events().map(|(_, size)| Message::WindowResized(size)),
    ])
}

fn keyboard_shortcut(
    event: iced::Event,
    status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    use keyboard::key::Named;
    use keyboard::{Event, Key};

    if status != iced::event::Status::Ignored {
        return None;
    }
    let iced::Event::Keyboard(Event::KeyPressed {
        key,
        modifiers,
        ..
    }) = event
    else {
        return None;
    };
    let key = key.as_ref();
    if modifiers.control() || modifiers.command() {
        return match key {
            Key::Character("i" | "I") => Some(Message::Shortcut(Shortcut::Import)),
            Key::Character(",") => Some(Message::Shortcut(Shortcut::Settings)),
            _ => None,
        };
    }
    if modifiers.alt() {
        return None;
    }
    let shortcut = match key {
        Key::Named(Named::Escape) => return Some(Message::Dismiss),
        Key::Named(Named::Space) => Shortcut::TogglePlayback,
        Key::Character("k" | "K") => Shortcut::TogglePlayback,
        Key::Character("j" | "J") => Shortcut::Back,
        Key::Character("l" | "L") => Shortcut::Forward,
        Key::Character("m" | "M") => Shortcut::MuteRange,
        Key::Character("b" | "B") | Key::Character("c" | "C") => Shortcut::Blade,
        Key::Character("a" | "A") | Key::Character("v" | "V") => Shortcut::DirectEdit,
        _ => return None,
    };
    Some(Message::Shortcut(shortcut))
}

/// Convenience for `main`: keeps the exit task type explicit.
pub fn quit() -> Task<Message> {
    iced::exit()
}

#[cfg(test)]
mod tests {
    use super::keyboard_shortcut;
    use crate::message::{Message, Shortcut};
    use iced::keyboard::{Event, Key};
    use iced::{event, keyboard, window};

    fn press(key: Key) -> Option<Message> {
        keyboard_shortcut(
            iced::Event::Keyboard(Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            }),
            event::Status::Ignored,
            window::Id::unique(),
        )
    }

    #[test]
    fn maps_transport_keys_to_shortcuts() {
        assert!(matches!(
            press(Key::Named(keyboard::key::Named::Space)),
            Some(Message::Shortcut(Shortcut::TogglePlayback))
        ));
        assert!(matches!(
            press(Key::Character("j".into())),
            Some(Message::Shortcut(Shortcut::Back))
        ));
        assert!(matches!(
            press(Key::Character("m".into())),
            Some(Message::Shortcut(Shortcut::MuteRange))
        ));
        assert!(matches!(
            press(Key::Named(keyboard::key::Named::Escape)),
            Some(Message::Dismiss)
        ));
    }

    #[test]
    fn ignores_keys_already_captured_by_text_inputs() {
        let message = keyboard_shortcut(
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(keyboard::key::Named::Space),
                modified_key: Key::Named(keyboard::key::Named::Space),
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            }),
            event::Status::Captured,
            window::Id::unique(),
        );
        assert!(message.is_none());
    }
}
