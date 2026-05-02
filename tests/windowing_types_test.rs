use costae::windowing::{MouseButton, WindowEvent};

#[test]
fn mouse_button_variants_are_distinct() {
    let left = MouseButton::Left;
    let middle = MouseButton::Middle;
    let right = MouseButton::Right;
    let other = MouseButton::Other(5);

    // Each variant must be constructible and distinguishable via pattern matching.
    assert!(matches!(left, MouseButton::Left));
    assert!(matches!(middle, MouseButton::Middle));
    assert!(matches!(right, MouseButton::Right));
    assert!(matches!(other, MouseButton::Other(5)));

    // Other(5) must not match Other(6).
    assert!(!matches!(other, MouseButton::Other(6)));
}

#[test]
fn window_event_click_carries_mouse_button_not_u32() {
    // Constructing WindowEvent::Click with a MouseButton value (not a raw u32).
    // This test fails to compile if `button` is typed as u32.
    let event = WindowEvent::Click {
        panel_id: "sidebar".to_string(),
        x_logical: 10.0,
        y_logical: 20.0,
        button: MouseButton::Left,
    };

    assert!(matches!(
        event,
        WindowEvent::Click {
            button: MouseButton::Left,
            ..
        }
    ));
}

#[test]
fn window_event_click_fields_are_named_x_logical_y_logical() {
    // Destructure all fields by name to confirm the renamed x_logical / y_logical fields.
    let event = WindowEvent::Click {
        panel_id: "top".to_string(),
        x_logical: 42.0,
        y_logical: 7.0,
        button: MouseButton::Other(9),
    };

    let WindowEvent::Click {
        panel_id,
        x_logical,
        y_logical,
        button,
    } = event
    else {
        panic!("expected WindowEvent::Click");
    };

    assert_eq!(panel_id, "top");
    assert_eq!(x_logical, 42.0_f32);
    assert_eq!(y_logical, 7.0_f32);
    assert!(matches!(button, MouseButton::Other(9)));
}
