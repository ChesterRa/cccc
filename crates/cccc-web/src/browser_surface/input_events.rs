//! CDP input events for browser-surface commands. Building them apart from
//! dispatch lets the event shapes be checked without launching a browser.

use anyhow::{Context, Result, bail};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    MouseButton,
};

/// The press and release that make one click with `button` at (x, y).
pub(super) fn click_events(
    x: f64,
    y: f64,
    button: &str,
) -> Result<(DispatchMouseEventParams, DispatchMouseEventParams)> {
    // `buttons` is the bitmask of buttons held down while the event fires.
    let (button, held) = match button {
        "left" => (MouseButton::Left, 1),
        "middle" => (MouseButton::Middle, 4),
        "right" => (MouseButton::Right, 2),
        value => bail!("unsupported browser mouse button: {value}"),
    };
    let mut pressed = DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, x, y);
    pressed.button = Some(button.clone());
    pressed.buttons = Some(held);
    pressed.click_count = Some(1);
    let mut released = DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, x, y);
    released.button = Some(button);
    released.buttons = Some(0);
    released.click_count = Some(1);
    Ok((pressed, released))
}

/// A wheel event at (x, y); the browser scrolls whatever is under that point.
pub(super) fn wheel_event(x: f64, y: f64, dx: f64, dy: f64) -> DispatchMouseEventParams {
    let mut wheel = DispatchMouseEventParams::new(DispatchMouseEventType::MouseWheel, x, y);
    wheel.delta_x = Some(dx);
    wheel.delta_y = Some(dy);
    wheel
}

/// Key down then key up for a named key. Keys that type a character send it
/// as text so inputs receive it; editing and navigation keys go down raw.
pub(super) fn key_events(key: &str) -> Result<[DispatchKeyEventParams; 2]> {
    let definition = chromiumoxide::keys::get_key_definition(key)
        .with_context(|| format!("unsupported browser key: {key}"))?;
    let mut command = DispatchKeyEventParams::builder()
        .key(definition.key)
        .code(definition.code)
        .windows_virtual_key_code(definition.key_code)
        .native_virtual_key_code(definition.key_code);
    let down_type = if let Some(text) = definition.text {
        command = command.text(text);
        DispatchKeyEventType::KeyDown
    } else if definition.key.len() == 1 {
        command = command.text(definition.key);
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };
    let down = command
        .clone()
        .r#type(down_type)
        .build()
        .map_err(anyhow::Error::msg)?;
    let up = command
        .r#type(DispatchKeyEventType::KeyUp)
        .build()
        .map_err(anyhow::Error::msg)?;
    Ok([down, up])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_presses_and_releases_the_requested_button_at_the_point() {
        for (name, button, held) in [
            ("left", MouseButton::Left, 1),
            ("middle", MouseButton::Middle, 4),
            ("right", MouseButton::Right, 2),
        ] {
            let (pressed, released) = click_events(12.5, 40.0, name).expect("supported button");
            assert_eq!(pressed.r#type, DispatchMouseEventType::MousePressed);
            assert_eq!(released.r#type, DispatchMouseEventType::MouseReleased);
            for event in [&pressed, &released] {
                assert_eq!((event.x, event.y), (12.5, 40.0));
                assert_eq!(event.button, Some(button.clone()), "{name}");
                assert_eq!(event.click_count, Some(1));
            }
            assert_eq!(pressed.buttons, Some(held), "{name} is held while pressed");
            assert_eq!(released.buttons, Some(0), "nothing is held after release");
        }
    }

    #[test]
    fn click_rejects_an_unknown_button() {
        let error = click_events(0.0, 0.0, "back").expect_err("unknown button");
        assert!(
            error
                .to_string()
                .contains("unsupported browser mouse button: back")
        );
    }

    #[test]
    fn wheel_carries_the_pointer_position_and_both_deltas() {
        let wheel = wheel_event(200.0, 150.0, -30.0, 240.0);
        assert_eq!(wheel.r#type, DispatchMouseEventType::MouseWheel);
        assert_eq!((wheel.x, wheel.y), (200.0, 150.0));
        assert_eq!((wheel.delta_x, wheel.delta_y), (Some(-30.0), Some(240.0)));
    }

    #[test]
    fn character_keys_type_text_and_navigation_keys_go_down_raw() {
        let [down, up] = key_events("a").expect("letter");
        assert_eq!(down.r#type, DispatchKeyEventType::KeyDown);
        assert_eq!(down.text.as_deref(), Some("a"));
        assert_eq!(up.r#type, DispatchKeyEventType::KeyUp);

        let [down, _] = key_events("Enter").expect("enter");
        assert_eq!(down.r#type, DispatchKeyEventType::KeyDown);
        assert_eq!(
            down.text.as_deref(),
            Some("\r"),
            "Enter submits as a typed return"
        );

        for key in ["Backspace", "ArrowDown", "Tab"] {
            let [down, up] = key_events(key).expect("named key");
            assert_eq!(down.r#type, DispatchKeyEventType::RawKeyDown, "{key}");
            assert_eq!(down.text, None, "{key} must not type text");
            assert_eq!(down.key.as_deref(), Some(key));
            assert_eq!(up.r#type, DispatchKeyEventType::KeyUp);
            assert_eq!(down.code, up.code);
            assert!(down.windows_virtual_key_code.is_some_and(|code| code > 0));
        }
    }

    #[test]
    fn unknown_keys_are_rejected_before_anything_is_sent() {
        let error = key_events("NotAKey").expect_err("unknown key");
        assert!(
            error
                .to_string()
                .contains("unsupported browser key: NotAKey")
        );
    }
}
