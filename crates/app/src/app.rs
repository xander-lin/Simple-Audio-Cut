//! Entry point: window configuration, subscriptions and keyboard shortcuts.

use std::time::Duration;

use iced::window;
use iced::{keyboard, Size, Subscription, Task};

use crate::message::{Message, Shortcut};
use crate::services::Services;
use crate::theme;
use crate::update::App;

/// Initial window size, shared by the window settings and the layout state.
pub const DEFAULT_WINDOW_SIZE: Size = Size::new(1280.0, 820.0);

/// Launches an application whose initial state is produced by `boot`.
///
/// Used by `examples/seed_preview` for visual inspection of the editor with
/// mock services and synthetic tracks.
pub fn preview(
    boot: impl Fn() -> App + Clone + Send + Sync + 'static,
) -> iced::Application<impl iced::Program<State = App, Message = Message, Theme = iced::Theme>> {
    // `boot` builds the state only; the monitor probe still has to run so the
    // layout can tell a full-screen window from a half-screen one.
    let boot = move || {
        let monitor = iced::window::latest()
            .and_then(iced::window::monitor_size)
            .map(Message::MonitorResolved);
        let mut app = boot();
        app.window_size = preview_size();
        (app, monitor)
    };
    iced::application(boot, App::update, App::view)
        .title("Simple Audio Cut — preview")
        .theme(|_: &App| theme::theme())
        .window(window::Settings {
            size: preview_size(),
            min_size: Some(Size::new(860.0, 620.0)),
            ..window::Settings::default()
        })
        .exit_on_close_request(false)
        .subscription(subscription)
}

/// Preview size, overridable so the two layouts can be inspected.
pub fn preview_size() -> Size {
    let read = |key: &str, fallback: f32| {
        std::env::var(key)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 320.0)
            .unwrap_or(fallback)
    };
    Size::new(
        read("PREVIEW_WIDTH", 1360.0),
        read("PREVIEW_HEIGHT", 880.0),
    )
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

    let files: Vec<std::path::PathBuf> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
        .collect();
    if !files.is_empty() {
        eprintln!(
            "simple-audio-cut: importing {} file argument(s)",
            files.len()
        );
    }

    iced::application(
        move || App::boot_with_files(services.clone(), files.clone()),
        App::update,
        App::view,
    )
    .title("Simple Audio Cut")
    .theme(|_: &App| theme::theme())
    .window(window::Settings {
        size: DEFAULT_WINDOW_SIZE,
        min_size: Some(Size::new(860.0, 620.0)),
        ..window::Settings::default()
    })
    .exit_on_close_request(false)
    .subscription(subscription)
    .run()
}

fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::batch([
        iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick),
        iced::event::listen_with(global_event),
        window::close_requests().map(|_| Message::CloseRequested),
        window::resize_events().map(|(_, size)| Message::WindowResized(size)),
    ])
}

/// Maps runtime events that are not widget-specific.
///
/// File drag-and-drop from the file manager arrives here; it is unavailable on
/// Wayland (winit limitation), where the in-app library drag is the way to add
/// takes.
fn global_event(
    event: iced::Event,
    status: iced::event::Status,
    window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::FileDropped(path)) => {
            return Some(Message::FilesDropped(vec![path]));
        }
        iced::Event::Window(window::Event::FileHovered(_)) => {
            return Some(Message::FilesHovered);
        }
        iced::Event::Window(window::Event::FilesHoveredLeft) => {
            return Some(Message::FilesHoverLeft);
        }
        _ => {}
    }
    keyboard_shortcut(event, status, window)
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
        Key::Named(Named::Enter) => Shortcut::AddSelected,
        Key::Named(Named::Delete) | Key::Named(Named::Backspace) => Shortcut::DeleteSelection,
        Key::Named(Named::Home) => Shortcut::JumpStart,
        Key::Named(Named::End) => Shortcut::JumpEnd,
        Key::Named(Named::Space) => Shortcut::TogglePlayback,
        Key::Character("k" | "K") => Shortcut::TogglePlayback,
        Key::Character("j" | "J") => Shortcut::Back,
        Key::Character("l" | "L") => Shortcut::Forward,
        Key::Character("m" | "M") => Shortcut::MuteRange,
        Key::Character("[" | "{") => Shortcut::ZoomOut,
        Key::Character("]" | "}") => Shortcut::ZoomIn,
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
