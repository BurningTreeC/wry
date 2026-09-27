---
"wry": patch
---

Don't disable the WebKitGTK input method preedit on Linux. `set_webview_settings`
used to call `webkit_input_method_context_set_enable_preedit(false)`
unconditionally, which breaks composition with CJK input methods (fcitx5 users
could not type Chinese/Japanese/Korean in the webview at all). The call was
added to keep the fcitx candidate window anchored at the caret, but current
webkit2gtk (>= 2.50) positions the popup correctly without it.
