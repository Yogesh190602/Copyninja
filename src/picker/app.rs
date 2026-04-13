use crate::config::Config;
use crate::content::ClipContent;
use crate::picker::css;
use crate::picker::paste;
use crate::storage::{self, ClipEntry};

use gio::prelude::*;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(config: &Config, target_is_terminal: bool) {
    let auto_paste = config.auto_paste;
    let app = gtk4::Application::builder()
        .application_id("com.copyninja.picker")
        .build();

    app.connect_activate(move |app| build_ui(app, auto_paste, target_is_terminal));
    app.run_with_args::<String>(&[]);
}

fn relative_time(timestamp: f64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let diff = (now - timestamp) as u64;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

fn build_ui(app: &gtk4::Application, auto_paste: bool, target_is_terminal: bool) {
    // -- Install CSS --
    let css_provider = gtk4::CssProvider::new();
    css_provider.load_from_data(css::THEME);
    gtk4::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // -- Shared state --
    let entries_map: Rc<RefCell<HashMap<String, ClipEntry>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let search_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // -- Window --
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Clipboard")
        .default_width(380)
        .default_height(500)
        .resizable(false)
        .build();

    // -- Headerbar --
    let headerbar = gtk4::HeaderBar::new();
    let title_label = gtk4::Label::new(Some("Clipboard"));
    title_label.add_css_class("title-label");

    let count_badge = gtk4::Label::new(Some("0"));
    count_badge.add_css_class("count-badge");

    let title_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    title_box.set_halign(gtk4::Align::Center);
    title_box.append(&title_label);
    title_box.append(&count_badge);
    headerbar.set_title_widget(Some(&title_box));
    window.set_titlebar(Some(&headerbar));

    // -- Main layout --
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // -- Search --
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search clipboard..."));
    search_entry.add_css_class("search-entry");
    vbox.append(&search_entry);

    // -- Scrolled list --
    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);

    let scrolled = gtk4::ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .build();
    vbox.append(&scrolled);

    // -- Footer --
    let footer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    footer.add_css_class("footer-bar");

    let clear_btn = gtk4::Button::with_label("Clear all");
    clear_btn.add_css_class("clear-btn");
    clear_btn.set_hexpand(true);
    footer.append(&clear_btn);
    vbox.append(&footer);

    window.set_child(Some(&vbox));

    // -- Populate the list --
    let count_badge_c = count_badge.clone();
    let list_box_c = list_box.clone();
    let entries_map_c = entries_map.clone();
    let populate = Rc::new(move || {
        populate_list(&list_box_c, &entries_map_c, &count_badge_c);
    });
    populate();

    // -- Setup app actions (pin, delete) --
    let populate_pin = populate.clone();
    let pin_action = gio::SimpleAction::new("toggle-pin", Some(glib::VariantTy::STRING));
    pin_action.connect_activate(move |_, param| {
        if let Some(param) = param {
            if let Some(hash) = param.get::<String>() {
                toggle_pin(&hash);
                populate_pin();
            }
        }
    });
    app.add_action(&pin_action);

    let populate_del = populate.clone();
    let delete_action = gio::SimpleAction::new("delete-entry", Some(glib::VariantTy::STRING));
    delete_action.connect_activate(move |_, param| {
        if let Some(param) = param {
            if let Some(hash) = param.get::<String>() {
                delete_entry(&hash);
                populate_del();
            }
        }
    });
    app.add_action(&delete_action);

    // -- Search filter --
    let search_text_filter = search_text.clone();
    let entries_map_filter = entries_map.clone();
    list_box.set_filter_func(move |row| {
        let query = search_text_filter.borrow().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let name = row.widget_name();
        if let Some(hash) = name.strip_prefix("entry:") {
            let map = entries_map_filter.borrow();
            if let Some(entry) = map.get(hash) {
                return entry.display_preview().to_lowercase().contains(&query);
            }
        }
        true // headers always visible
    });

    let search_text_c = search_text.clone();
    let list_box_search = list_box.clone();
    search_entry.connect_search_changed(move |entry| {
        *search_text_c.borrow_mut() = entry.text().to_string();
        list_box_search.invalidate_filter();
    });

    // -- Shared pasting flag (prevents focus-loss handler from quitting mid-paste) --
    let is_pasting: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // -- Row activation: copy + paste --
    let app_hold = app.clone();
    let window_activate = window.clone();
    let is_pasting_activate = is_pasting.clone();
    list_box.connect_row_activated(move |_, row| {
        let name = row.widget_name();
        if !name.starts_with("entry:") {
            return;
        }

        let hash = &name[6..];
        let entries = entries_map.borrow();
        let entry = match entries.get(hash) {
            Some(e) => e.clone(),
            None => return,
        };
        let content = entry.resolved_content();
        drop(entries);

        // Write to clipboard based on content type.
        // For images, force non-terminal paste mode: Ctrl+Shift+V never pastes
        // images in any common app (terminals don't accept images, GTK apps
        // treat it as Unicode entry, image editors ignore it). Ctrl+V is the
        // only shortcut that actually pastes image data.
        let (text_for_paste, paste_as_terminal) = match &content {
            ClipContent::Text { text, .. } => {
                if !paste::write_clipboard_sync(text) {
                    if let Some(display) = gdk4::Display::default() {
                        let clipboard = display.clipboard();
                        clipboard.set_text(text);
                    }
                }
                (text.clone(), target_is_terminal)
            }
            ClipContent::Image { path, mime } => {
                paste::write_image_clipboard_sync(path, mime);
                (String::new(), false) // always Ctrl+V for images
            }
        };

        *is_pasting_activate.borrow_mut() = true;
        window_activate.set_visible(false);

        if auto_paste {
            // Hold the application alive while we paste (guard dropped on quit)
            let guard = app_hold.upcast_ref::<gio::Application>().hold();

            // Paste in a background thread — simulate_paste polls for focus
            // loss internally, so no fixed delay needed here.
            let app_quit = app_hold.clone();
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                paste::simulate_paste(&text_for_paste, paste_as_terminal);
                let _ = tx.send(());
            });
            // Poll from GLib main loop until paste thread completes
            let mut guard = Some(guard);
            glib::timeout_add_local(Duration::from_millis(50), move || {
                if rx.try_recv().is_ok() {
                    let app_quit2 = app_quit.clone();
                    let guard = guard.take();
                    glib::timeout_add_local_once(Duration::from_millis(200), move || {
                        drop(guard);
                        app_quit2.quit();
                    });
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        } else {
            // Auto-paste disabled — just quit after copying to clipboard
            app_hold.quit();
        }
    });

    // -- Clear all (two-step confirmation) --
    let clear_pending: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let populate_clear = populate.clone();
    let clear_pending_click = clear_pending.clone();
    let clear_btn_ref = clear_btn.clone();
    clear_btn.connect_clicked(move |btn| {
        if *clear_pending_click.borrow() {
            // Second click: clear unpinned entries
            clear_unpinned();
            *clear_pending_click.borrow_mut() = false;
            btn.set_label("Clear all");
            btn.remove_css_class("confirm");
            populate_clear();
        } else {
            // First click: ask for confirmation
            *clear_pending_click.borrow_mut() = true;
            btn.set_label("Confirm clear");
            btn.add_css_class("confirm");

            // Reset after 3 seconds
            let pending = clear_pending_click.clone();
            let btn_c = clear_btn_ref.clone();
            glib::timeout_add_local_once(Duration::from_secs(3), move || {
                if *pending.borrow() {
                    *pending.borrow_mut() = false;
                    btn_c.set_label("Clear all");
                    btn_c.remove_css_class("confirm");
                }
            });
        }
    });

    // -- Keyboard shortcuts --
    let key_controller = gtk4::EventControllerKey::new();
    let app_key = app.clone();
    let list_box_key = list_box.clone();
    key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
        let ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);

        match keyval {
            gdk4::Key::Escape => {
                app_key.quit();
                glib::Propagation::Stop
            }
            gdk4::Key::Return | gdk4::Key::KP_Enter => {
                if let Some(row) = list_box_key.selected_row() {
                    list_box_key.emit_by_name::<()>("row-activated", &[&row]);
                }
                glib::Propagation::Stop
            }
            gdk4::Key::p | gdk4::Key::P if ctrl => {
                if let Some(row) = list_box_key.selected_row() {
                    let name = row.widget_name();
                    if let Some(hash) = name.strip_prefix("entry:") {
                        let _ = app_key.activate_action(
                            "toggle-pin",
                            Some(&hash.to_variant()),
                        );
                    }
                }
                glib::Propagation::Stop
            }
            gdk4::Key::d | gdk4::Key::D if ctrl => {
                if let Some(row) = list_box_key.selected_row() {
                    let name = row.widget_name();
                    if let Some(hash) = name.strip_prefix("entry:") {
                        let _ = app_key.activate_action(
                            "delete-entry",
                            Some(&hash.to_variant()),
                        );
                    }
                }
                glib::Propagation::Stop
            }
            gdk4::Key::l | gdk4::Key::L if ctrl => {
                clear_btn.emit_clicked();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_controller);

    // -- Focus-loss close (with 500ms grace period) --
    let focus_close_enabled: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let fce = focus_close_enabled.clone();
    glib::timeout_add_local_once(Duration::from_millis(500), move || {
        *fce.borrow_mut() = true;
    });

    let fce = focus_close_enabled.clone();
    let app_focus = app.clone();
    let is_pasting_focus = is_pasting.clone();
    window.connect_is_active_notify(move |win| {
        if !win.is_active() && *fce.borrow() && !*is_pasting_focus.borrow() {
            let win_c = win.clone();
            let app_c = app_focus.clone();
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if !win_c.is_active() {
                    app_c.quit();
                }
            });
        }
    });

    // -- Present --
    window.present();
    search_entry.grab_focus();
}

fn populate_list(
    list_box: &gtk4::ListBox,
    entries_map: &Rc<RefCell<HashMap<String, ClipEntry>>>,
    count_badge: &gtk4::Label,
) {
    // Remove all existing children
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let history = storage::load_history();
    count_badge.set_label(&history.len().to_string());

    if history.is_empty() {
        show_empty_state(list_box);
        return;
    }

    let mut map = entries_map.borrow_mut();
    map.clear();

    let pinned: Vec<_> = history.iter().filter(|e| e.pinned).cloned().collect();
    let recent: Vec<_> = history.iter().filter(|e| !e.pinned).cloned().collect();

    if !pinned.is_empty() {
        list_box.append(&build_section_header("PINNED"));
        for entry in &pinned {
            map.insert(entry.hash.clone(), entry.clone());
            list_box.append(&build_row(entry));
        }
    }

    if !recent.is_empty() {
        if !pinned.is_empty() {
            list_box.append(&build_section_header("RECENT"));
        }
        for entry in &recent {
            map.insert(entry.hash.clone(), entry.clone());
            list_box.append(&build_row(entry));
        }
    }

    // Select first selectable row
    if let Some(row) = list_box.row_at_index(if pinned.is_empty() { 0 } else { 1 }) {
        list_box.select_row(Some(&row));
    }
}

fn show_empty_state(list_box: &gtk4::ListBox) {
    let row = gtk4::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_widget_name("header");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    vbox.set_halign(gtk4::Align::Center);
    vbox.set_valign(gtk4::Align::Center);
    vbox.set_margin_top(80);
    vbox.set_margin_bottom(80);

    let icon = gtk4::Label::new(Some("\u{1f4cb}")); // clipboard emoji
    icon.add_css_class("empty-icon");

    let label = gtk4::Label::new(Some("Nothing here yet"));
    label.add_css_class("empty-label");

    let sublabel = gtk4::Label::new(Some("Copy something to get started"));
    sublabel.add_css_class("empty-sublabel");

    vbox.append(&icon);
    vbox.append(&label);
    vbox.append(&sublabel);
    row.set_child(Some(&vbox));
    list_box.append(&row);
}

fn build_section_header(text: &str) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_widget_name("header");

    let label = gtk4::Label::new(Some(text));
    label.add_css_class("section-header");
    label.set_halign(gtk4::Align::Start);
    row.set_child(Some(&label));
    row
}

fn build_row(entry: &ClipEntry) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    row.set_widget_name(&format!("entry:{}", entry.hash));

    let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    card.add_css_class("clip-card");

    // Pin icon
    if entry.pinned {
        let pin_label = gtk4::Label::new(Some("\u{1f4cc}")); // pin emoji
        pin_label.add_css_class("pin-icon");
        pin_label.set_valign(gtk4::Align::Center);
        card.append(&pin_label);
    }

    // Content column
    let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    content_box.set_hexpand(true);

    let resolved = entry.resolved_content();
    match &resolved {
        ClipContent::Text { preview: prev, .. } => {
            let preview = gtk4::Label::new(Some(prev));
            preview.add_css_class("clip-preview");
            preview.set_halign(gtk4::Align::Start);
            preview.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            preview.set_max_width_chars(40);
            preview.set_single_line_mode(true);
            content_box.append(&preview);
        }
        ClipContent::Image { path, mime } => {
            // Show image thumbnail
            let image_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            if path.exists() {
                let picture = gtk4::Picture::for_filename(path);
                picture.set_size_request(48, 48);
                picture.add_css_class("clip-image");
                image_box.append(&picture);
            }
            let type_label = gtk4::Label::new(Some(mime));
            type_label.add_css_class("clip-preview");
            type_label.set_halign(gtk4::Align::Start);
            image_box.append(&type_label);
            content_box.append(&image_box);
        }
    }

    let time_label = gtk4::Label::new(Some(&relative_time(entry.time)));
    time_label.add_css_class("clip-time");
    time_label.set_halign(gtk4::Align::Start);

    content_box.append(&time_label);
    card.append(&content_box);

    // Menu button
    let menu_model = gio::Menu::new();

    let pin_text = if entry.pinned { "Unpin" } else { "Pin" };
    let pin_item = gio::MenuItem::new(Some(pin_text), None);
    pin_item.set_action_and_target_value(
        Some("app.toggle-pin"),
        Some(&entry.hash.to_variant()),
    );
    menu_model.append_item(&pin_item);

    let delete_item = gio::MenuItem::new(Some("Delete"), None);
    delete_item.set_action_and_target_value(
        Some("app.delete-entry"),
        Some(&entry.hash.to_variant()),
    );
    menu_model.append_item(&delete_item);

    let menu_btn = gtk4::MenuButton::new();
    menu_btn.set_menu_model(Some(&menu_model));
    menu_btn.set_icon_name("view-more-symbolic");
    menu_btn.add_css_class("menu-btn");
    menu_btn.set_valign(gtk4::Align::Center);
    card.append(&menu_btn);

    row.set_child(Some(&card));
    row
}

fn toggle_pin(hash: &str) {
    let mut history = storage::load_history();
    if let Some(entry) = history.iter_mut().find(|e| e.hash == hash) {
        entry.pinned = !entry.pinned;
    }
    let _ = storage::save_history(&history);
}

fn delete_entry(hash: &str) {
    let mut history = storage::load_history();
    // Clean up image file if present
    if let Some(entry) = history.iter().find(|e| e.hash == hash) {
        if let ClipContent::Image { path, .. } = entry.resolved_content() {
            let _ = std::fs::remove_file(path);
        }
    }
    history.retain(|e| e.hash != hash);
    let _ = storage::save_history(&history);

    // Write tombstone so sync doesn't re-import this entry
    let config = crate::config::load();
    if config.sync.enabled {
        crate::sync::write_tombstone(&config.sync.sync_dir, hash);
    }
}

fn clear_unpinned() {
    let history = storage::load_history();
    // Clean up image files for unpinned entries
    for entry in history.iter().filter(|e| !e.pinned) {
        if let ClipContent::Image { path, .. } = entry.resolved_content() {
            let _ = std::fs::remove_file(path);
        }
    }
    let pinned_only: Vec<_> = history.into_iter().filter(|e| e.pinned).collect();
    let _ = storage::save_history(&pinned_only);
}
