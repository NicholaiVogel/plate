//! Small AppKit helpers for the integrated header and native interaction checks.
// objc's selector macros contain a legacy cargo-clippy configuration check.
#![allow(unexpected_cfgs)]
use cocoa::foundation::{NSPoint, NSRect, NSInteger, NSUInteger};
use gpui::{Pixels, Point, Window};
use objc::{class, msg_send, sel, sel_impl, runtime::Object};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn native_window(window: &Window) -> *mut Object {
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).expect("Mac window handle").as_raw() else { unreachable!() };
    // SAFETY: GPUI owns a live AppKit view for the lifetime of Window.
    unsafe { msg_send![handle.ns_view.as_ptr() as *mut Object, window] }
}

pub fn start_header_drag(window: &Window) {
    // SAFETY: called on the UI thread in a native mouse-down callback.
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![app, currentEvent];
        if !event.is_null() {
            let _: () = msg_send![native_window(window), performSelector: sel!(performWindowDragWithEvent:)
                withObject: event afterDelay: 0.0f64];
        }
    }
}

/// Send an event through AppKit and GPUI without requesting OS input access.
pub fn check_mouse_event(window: &Window, event_type: NSUInteger, position: Point<Pixels>) {
    // SAFETY: all objects belong to this app, and calls run on the UI thread.
    unsafe {
        let native = native_window(window);
        let number: NSInteger = msg_send![native, windowNumber];
        let location = NSPoint::new(f32::from(position.x) as f64,
            f32::from(window.viewport_size().height - position.y) as f64);
        let event: *mut Object = msg_send![class!(NSEvent),
            mouseEventWithType: event_type location: location modifierFlags: 0usize
            timestamp: 0.0f64 windowNumber: number context: std::ptr::null_mut::<Object>()
            eventNumber: 0isize clickCount: 1isize pressure: 1.0f32];
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        // Send on the next run-loop turn, after GPUI releases its App borrow.
        let _: () = msg_send![app, performSelector: sel!(sendEvent:)
            withObject: event afterDelay: 0.0f64];
    }
}

pub fn check_header(window: &Window) {
    // SAFETY: inspect this app's live AppKit window on the UI thread.
    unsafe {
        let native = native_window(window);
        let mask: NSUInteger = msg_send![native, styleMask];
        let visibility: NSInteger = msg_send![native, titleVisibility];
        assert_ne!(mask & (1 << 15), 0, "content must extend into the titlebar");
        assert_eq!(visibility, 1, "system window title must be hidden");
        let content: *mut Object = msg_send![native, contentView];
        for kind in [0usize, 1, 2] {
            let button: *mut Object = msg_send![native, standardWindowButton: kind];
            let frame: NSRect = msg_send![button, frame];
            let parent: *mut Object = msg_send![button, superview];
            let rect: NSRect = msg_send![parent, convertRect: frame toView: content];
            let top = f32::from(window.viewport_size().height) as f64 - rect.origin.y - rect.size.height;
            assert!(top >= 0.0 && top + rect.size.height <= f32::from(crate::theme::s(48.)) as f64,
                "traffic lights must lie within Plate's header: y={top}");
            assert!(rect.origin.x >= 0.0 && rect.origin.x + rect.size.width < 84.0,
                "traffic lights must stay in the reserved header space");
        }
        println!("PASS: native traffic lights are inside Plate's header");
    }
}
