# TiddlyDesktop WRY Fork Changes

This fork of WRY (v0.53.5) contains modifications for TiddlyDesktop's drag-drop and input handling requirements.

## Overview

The main changes enable:
1. **Composition Hosting Mode** on Windows for full drag-drop control
2. **Pointer Event Support** (touch, pen, stylus) on Windows
3. **Native Drop Handling** on Linux/macOS for text insertion into inputs

## Files Modified

### 1. `src/webview2/mod.rs` (Windows)

#### New Imports
```rust
// DirectComposition for composition hosting
use windows::Win32::Graphics::{
  Direct3D::D3D_DRIVER_TYPE_HARDWARE,
  Direct3D11::{D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION},
  DirectComposition::{DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual},
  Dxgi::IDXGIDevice,
};

// Pointer events for touch/pen support
use windows::Win32::UI::Input::Pointer::{
  GetPointerInfo, GetPointerPenInfo, GetPointerTouchInfo,
  POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
};
```

#### InnerWebView Struct Changes
Add fields for composition hosting:
```rust
pub(crate) struct InnerWebView {
  // ... existing fields ...

  // Composition hosting fields
  composition_controller: Option<ICoreWebView2CompositionController>,
  dcomp_device: Option<IDCompositionDevice>,
  env_for_pointer: Option<ICoreWebView2Environment3>,

  // ... rest of fields ...
}
```

#### New Function: `create_composition_controller`
Creates a composition controller instead of windowed controller:
- Creates D3D11 device with BGRA support
- Creates DirectComposition device, visual, and target
- Calls `CreateCoreWebView2CompositionController` instead of `CreateCoreWebView2Controller`
- Sets the visual as root and commits

#### New Functions: Accessors
```rust
pub fn composition_controller(&self) -> Option<&ICoreWebView2CompositionController>
pub fn env_for_pointer(&self) -> Option<&ICoreWebView2Environment3>
```

#### Modified: `new_in_hwnd`
- Calls `create_composition_controller` instead of `create_controller`
- Sets `AllowExternalDrop(true)` instead of `false`
- Stores composition controller fields

#### Modified: `parent_subclass_proc`
Add input forwarding for composition hosting:

**Mouse Events** (via `SendMouseInput`):
- `WM_LBUTTONDOWN/UP`, `WM_RBUTTONDOWN/UP`, `WM_MBUTTONDOWN/UP`
- `WM_XBUTTONDOWN/UP`, `WM_MOUSEMOVE`, `WM_MOUSELEAVE`
- `WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`
- `WM_*DBLCLK` for double-clicks

**Pointer Events** (via `SendPointerInput` with full `ICoreWebView2PointerInfo`):
- `WM_POINTERDOWN`, `WM_POINTERUP`, `WM_POINTERUPDATE`
- `WM_POINTERENTER`, `WM_POINTERLEAVE`
- `WM_POINTERWHEEL`, `WM_POINTERHWHEEL`
- `WM_POINTERACTIVATE`, `WM_POINTERCAPTURECHANGED`

Full touch/pen metadata is preserved:
- Touch: contact area, pressure, orientation
- Pen: pressure, tilt X/Y, rotation

**IME Composition** (for international text input):
- `WM_IME_STARTCOMPOSITION`, `WM_IME_COMPOSITION`, `WM_IME_ENDCOMPOSITION`
- `WM_IME_SETCONTEXT`, `WM_IME_NOTIFY`, `WM_IME_CHAR`
- `WM_IME_REQUEST`, `WM_IME_KEYDOWN`, `WM_IME_KEYUP`
- `WM_IME_COMPOSITIONFULL`, `WM_IME_SELECT`

**Keyboard Events**:
- `WM_KEYDOWN`, `WM_KEYUP`, `WM_CHAR`, `WM_DEADCHAR`
- `WM_SYSKEYDOWN`, `WM_SYSKEYUP`, `WM_SYSCHAR`, `WM_SYSDEADCHAR`
- `WM_UNICHAR` for Unicode input
- Ensures WebView2 focus for keyboard input

**Touch Gestures**:
- `WM_GESTURE` - pinch-to-zoom, pan, rotate
- `WM_GESTURENOTIFY` - gesture configuration
- `WM_TOUCH` - legacy touch input for compatibility

**Mouse Capture**:
- `WM_CAPTURECHANGED` - resets mouse state when capture is lost

**Non-Client Area Mouse** (window chrome):
- `WM_NCMOUSEMOVE`, `WM_NCLBUTTONDOWN/UP`, `WM_NCRBUTTONDOWN/UP`
- `WM_NCMBUTTONDOWN/UP`, `WM_NCXBUTTONDOWN/UP`
- All double-click variants for non-client area

**Context Menu**:
- `WM_CONTEXTMENU` - ensures WebView2 focus for context menu handling

**DPI Changes**:
- `WM_DPICHANGED` - updates WebView2 bounds and notifies of DPI change

**Focus & Activation**:
- `WM_ACTIVATE` - gives focus to WebView2 when window activates
- `WM_MOUSEACTIVATE` - ensures focus on mouse click activation
- `WM_KILLFOCUS` - handled internally by WebView2

**Mouse Tracking**:
- Registers `TrackMouseEvent` with `TME_LEAVE` for `WM_MOUSELEAVE` notifications

**Cursor Handling**:
- `WM_SETCURSOR`: Get cursor from composition controller and set it

#### Helper Functions for Pointer Input
```rust
// Fill ICoreWebView2PointerInfo from Windows pointer data
unsafe fn fill_touch_pointer_info(pointer_id: u32, info: &ICoreWebView2PointerInfo, hwnd: HWND) -> bool;
unsafe fn fill_pen_pointer_info(pointer_id: u32, info: &ICoreWebView2PointerInfo, hwnd: HWND) -> bool;
unsafe fn fill_mouse_pointer_info(pointer_id: u32, info: &ICoreWebView2PointerInfo, hwnd: HWND) -> bool;
unsafe fn fill_generic_pointer_info(pointer_id: u32, info: &ICoreWebView2PointerInfo, hwnd: HWND) -> bool;
```

These functions extract full pointer metadata from Windows APIs:
- `GetPointerTouchInfo()` for touch (pressure, contact area, orientation)
- `GetPointerPenInfo()` for pen (pressure, tilt, rotation)
- `GetPointerInfo()` for generic/mouse (basic pointer data)

### 2. `src/webkitgtk/drag_drop.rs` (Linux)

#### Modified: `connect_drag_drop` handler
The original code intercepts drops and prevents WebKit from handling them. The modification allows WebKit to handle all drops natively while still emitting events:

```rust
webview.connect_drag_drop(move |_, ctx, x, y, time| {
  // Let WebKit handle ALL drops natively
  // This preserves text insertion into inputs/textareas/contenteditables

  // For internal drops, just let WebKit handle
  if ctx.drag_get_source_widget().is_some() {
    return false;
  }

  // For external drops with file paths, emit the event but still return false
  if controller.state() == DragControllerState::Leaving {
    if let Some(paths) = controller.take_paths() {
      controller.leave();
      controller.call(DragDropEvent::Drop { paths, position: (x, y) });
      return false;  // Let WebKit also handle it
    }
  }

  false
});
```

### 3. `src/wkwebview/drag_drop.rs` (macOS)

#### Modified: `perform_drag_operation`
Add FFI hooks for TiddlyDesktop to:
1. Check if it's an internal drag (from the same app)
2. Store file paths for JavaScript to retrieve
3. Fix pasteboard data for internal drags

```rust
pub(crate) fn perform_drag_operation(...) -> Bool {
  // Check for internal drags via FFI
  let is_internal_drag = unsafe {
    extern "C" { fn tiddlydesktop_has_internal_drag() -> i32; }
    tiddlydesktop_has_internal_drag() != 0
  };

  // For external file drops, store paths via FFI
  if !is_internal_drag && !paths.is_empty() {
    unsafe {
      extern "C" { fn tiddlydesktop_store_drop_paths(paths_json: *const c_char); }
      // Convert paths to JSON and store
    }
  }

  // For internal drags, fix pasteboard data
  if is_internal_drag {
    unsafe {
      extern "C" {
        fn tiddlydesktop_get_internal_drag_text_plain() -> *const c_char;
        fn tiddlydesktop_get_internal_drag_tiddler_json() -> *const c_char;
      }
      // Fix text/plain and text/vnd.tiddler on pasteboard
    }
  }

  // Always invoke native WKWebView handling
  unsafe { objc2::msg_send![super(this), performDragOperation: drag_info] }
}
```

## Why These Changes?

### Windows Composition Hosting
In standard windowed mode, WebView2 creates its own Chrome_WidgetWin_* windows and handles drag-drop internally. The IDropTarget is in the browser process and cannot be intercepted.

With composition hosting:
- Application provides the DirectComposition visual for rendering
- Application registers IDropTarget on parent HWND
- Application forwards drag events via `ICoreWebView2CompositionController3::DragEnter/Drop`
- Full control over drag-drop with file path extraction

### Pointer Events with Full Metadata
Composition hosting requires explicit input forwarding. Mouse events alone aren't sufficient for:
- Touch screens (multi-touch, gestures, contact area)
- Stylus/pen input (pressure, tilt X/Y, rotation, eraser)
- Pointer capture handling

Using `SendPointerInput` with properly filled `ICoreWebView2PointerInfo` preserves all this metadata, enabling:
- Pressure-sensitive drawing applications
- Palm rejection based on contact area
- Pen tilt for calligraphy effects

### IME Support
Composition hosting requires IME message handling for international text input:
- Chinese (Pinyin, Wubi, etc.)
- Japanese (Hiragana, Katakana, Kanji)
- Korean (Hangul)
- Other input methods (Vietnamese, Thai, etc.)

The implementation forwards IME messages and sets the composition window position.

### Native Drop Handling (Linux/macOS)
WebKit's native drop handling correctly inserts text into inputs/textareas. The original WRY code intercepted drops and prevented this. The modifications:
- Let WebKit handle the drop natively
- Still emit events so TiddlyDesktop knows about the drop
- Enable file path extraction via FFI hooks

## Required FFI Functions (macOS)

TiddlyDesktop must provide these C functions:
```c
int32_t tiddlydesktop_has_internal_drag(void);
void tiddlydesktop_store_drop_paths(const char* paths_json);
const char* tiddlydesktop_get_internal_drag_text_plain(void);
const char* tiddlydesktop_get_internal_drag_tiddler_json(void);
```

## Cargo.toml Dependencies

The Windows build requires additional features:
```toml
[target.'cfg(target_os = "windows")'.dependencies.windows]
features = [
  # ... existing features ...
  "Win32_Graphics_Direct3D",
  "Win32_Graphics_Direct3D11",
  "Win32_Graphics_DirectComposition",
  "Win32_Graphics_Dxgi",
  "Win32_UI_Input_Pointer",
  "Win32_UI_Input_Ime",  # IME support for international text input
]
```

## Compatibility

- WRY base version: 0.53.5
- Windows: Requires Edge 88+ (composition hosting support)
- Linux: No additional requirements
- macOS: Requires TiddlyDesktop FFI functions to be linked
