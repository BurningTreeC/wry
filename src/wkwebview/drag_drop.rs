// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{ffi::{CStr, CString}, path::PathBuf};

use objc2::{
  runtime::{Bool, ProtocolObject},
  DeclaredClass,
};
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSFilenamesPboardType};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSString};

use crate::DragDropEvent;

use super::WryWebView;

pub(crate) unsafe fn collect_paths(drag_info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<PathBuf> {
  let pb = drag_info.draggingPasteboard();
  let mut drag_drop_paths = Vec::new();
  let types = NSArray::arrayWithObject(NSFilenamesPboardType);

  if pb.availableTypeFromArray(&types).is_some() {
    let paths = pb.propertyListForType(NSFilenamesPboardType).unwrap();
    let paths = paths.downcast::<NSArray>().unwrap();
    for path in paths {
      let path = path.downcast::<NSString>().unwrap();
      let path = CStr::from_ptr(path.UTF8String()).to_string_lossy();
      drag_drop_paths.push(PathBuf::from(path.into_owned()));
    }
  }
  drag_drop_paths
}

pub(crate) fn dragging_entered(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
  let paths = unsafe { collect_paths(drag_info) };
  let dl: NSPoint = unsafe { drag_info.draggingLocation() };
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  let listener = &this.ivars().drag_drop_handler;
  if !listener(DragDropEvent::Enter { paths, position }) {
    // Reject the Wry file drop (invoke the OS default behaviour)
    unsafe { objc2::msg_send![super(this), draggingEntered: drag_info] }
  } else {
    NSDragOperation::Copy
  }
}

pub(crate) fn dragging_updated(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
  let dl: NSPoint = unsafe { drag_info.draggingLocation() };
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  let listener = &this.ivars().drag_drop_handler;
  if !listener(DragDropEvent::Over { position }) {
    unsafe {
      let os_operation = objc2::msg_send![super(this), draggingUpdated: drag_info];
      if os_operation == NSDragOperation::None {
        // 0 will be returned for a drop on any arbitrary location on the webview.
        // We'll override that with NSDragOperationCopy.
        NSDragOperation::Copy
      } else {
        // A different NSDragOperation is returned when a file is hovered over something like
        // a <input type="file">, so we'll make sure to preserve that behaviour.
        os_operation
      }
    }
  } else {
    NSDragOperation::Copy
  }
}

pub(crate) fn perform_drag_operation(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> Bool {
  let paths = unsafe { collect_paths(drag_info) };
  let dl: NSPoint = unsafe { drag_info.draggingLocation() };
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  // TiddlyDesktop: Check for internal drags BEFORE emitting events
  let is_internal_drag = unsafe {
    extern "C" {
      fn tiddlydesktop_has_internal_drag() -> i32;
    }
    tiddlydesktop_has_internal_drag() != 0
  };

  // TiddlyDesktop: For external file drops, store paths via FFI for JavaScript to retrieve.
  // This allows native HTML5 drop events to fire, and JS retrieves paths afterward.
  // DON'T call the listener for file drops - that would cause duplicate processing.
  if !is_internal_drag && !paths.is_empty() {
    unsafe {
      extern "C" {
        fn tiddlydesktop_store_drop_paths(paths_json: *const std::ffi::c_char);
      }
      // Convert paths to JSON array string
      let json_parts: Vec<String> = paths
        .iter()
        .map(|p| {
          let s = p.to_string_lossy();
          let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
          format!("\"{}\"", escaped)
        })
        .collect();
      let json = format!("[{}]", json_parts.join(","));
      if let Ok(cstr) = std::ffi::CString::new(json) {
        tiddlydesktop_store_drop_paths(cstr.as_ptr());
      }
    }
    // Don't call listener - let native handling fire HTML5 events
  }

  // For external drops WITHOUT file paths (text, html, url), still call listener
  // so our td-drag-content event system can handle them
  if !is_internal_drag && paths.is_empty() {
    let listener = &this.ivars().drag_drop_handler;
    listener(DragDropEvent::Drop {
      paths: paths.clone(),
      position,
    });
  }

  // TiddlyDesktop: For internal drags, fix the pasteboard data before native handling.
  // This ensures:
  // 1. Inputs receive the correct text (tiddler title) instead of the resolved URL
  // 2. TiddlyWiki dropzones receive the full tiddler JSON (text/vnd.tiddler)
  if is_internal_drag {
    unsafe {
      extern "C" {
        fn tiddlydesktop_get_internal_drag_text_plain() -> *const std::ffi::c_char;
        fn tiddlydesktop_get_internal_drag_tiddler_json() -> *const std::ffi::c_char;
      }

      // Get the pasteboard
      let pasteboard: *mut objc2::runtime::AnyObject =
        objc2::msg_send![drag_info, draggingPasteboard];
      if !pasteboard.is_null() {
        // Fix text/plain (for native input insertion)
        let text_ptr = tiddlydesktop_get_internal_drag_text_plain();
        if !text_ptr.is_null() {
          let text_cstr = std::ffi::CStr::from_ptr(text_ptr);
          if let Ok(text_str) = text_cstr.to_str() {
            let ns_string = NSString::from_str(text_str);
            let type_string = NSString::from_str("public.utf8-plain-text");
            let _: () = objc2::msg_send![pasteboard, setString: &*ns_string, forType: &*type_string];
          }
        }

        // Fix text/vnd.tiddler (for TiddlyWiki dropzone handlers)
        let tiddler_ptr = tiddlydesktop_get_internal_drag_tiddler_json();
        if !tiddler_ptr.is_null() {
          let tiddler_cstr = std::ffi::CStr::from_ptr(tiddler_ptr);
          if let Ok(tiddler_str) = tiddler_cstr.to_str() {
            let ns_string = NSString::from_str(tiddler_str);
            let type_string = NSString::from_str("text/vnd.tiddler");
            let _: () = objc2::msg_send![pasteboard, setString: &*ns_string, forType: &*type_string];
          }
        }
      }
    }
  }

  // TiddlyDesktop: Always invoke native WKWebView handling
  // This allows text/file paths to be inserted into inputs natively
  unsafe { objc2::msg_send![super(this), performDragOperation: drag_info] }
}

pub(crate) fn dragging_exited(this: &WryWebView, drag_info: &ProtocolObject<dyn NSDraggingInfo>) {
  let listener = &this.ivars().drag_drop_handler;
  if !listener(DragDropEvent::Leave) {
    // Reject the Wry drop (invoke the OS default behaviour)
    unsafe { objc2::msg_send![super(this), draggingExited: drag_info] }
  }
}
