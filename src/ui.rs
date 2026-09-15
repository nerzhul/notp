use crate::otp::{current_timestamp, generate_code, remaining_seconds, Algorithm};
use crate::qr_import::OtpParams;
use crate::settings::{
    AppSettings, Theme, MAX_AUTO_LOCK_SECONDS, MAX_CLIPBOARD_CLEAR_SECONDS, MIN_AUTO_LOCK_SECONDS,
    MIN_CLIPBOARD_CLEAR_SECONDS,
};
use crate::storage::{Account, Vault, VaultStore, CURRENT_VAULT_VERSION};
use gtk::gdk::{ContentProvider, DragAction};
use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, ButtonsType,
    ComboBoxText, CssProvider, Dialog, DialogFlags, DragSource, DrawingArea, DropTarget, Entry,
    EventControllerKey, Grid, HeaderBar, Label, ListBox, ListBoxRow, MenuButton, MessageDialog,
    Orientation, Overlay, Paned, Popover, ResponseType, ScrolledWindow, SelectionMode,
    Separator, SpinButton, Spinner, Stack, WidgetPaintable,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use uuid::Uuid;

const APPLICATION_ID: &str = "com.nerzhul.notp";

pub fn run() {
    gtk::init().expect("Unable to initialize GTK");
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .build();

    application.connect_activate(move |app| {
        show_load_window(app, None);
    });

    application.run();
}

fn show_load_window(application: &Application, window_to_destroy: Option<ApplicationWindow>) {
    let settings = AppSettings::load().unwrap_or_default();
    let default_path = VaultStore::default_path()
        .unwrap_or_else(|_| PathBuf::from("vault.notp"));
    let initial_path = settings
        .last_vault_path
        .clone()
        .unwrap_or_else(|| default_path.clone());
    let has_saved_path = settings.last_vault_path.is_some();

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Notp — Open vault")
        .default_width(540)
        .default_height(340)
        .build();

    let dialog = Dialog::new();
    dialog.set_title(Some("Open vault"));
    dialog.set_transient_for(Some(&window));
    dialog.set_modal(true);
    dialog.set_default_size(520, 300);
    dialog.set_resizable(false);

    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    let explanation = Label::new(Some(
        "Choose a vault file and enter its master password. A new vault will be created if the file does not exist yet.",
    ));
    explanation.set_wrap(true);
    explanation.set_xalign(0.0);
    content.append(&explanation);

    let path_label = Label::new(Some("Vault file"));
    path_label.set_xalign(0.0);
    path_label.set_margin_top(4);
    content.append(&path_label);

    let path_row = GtkBox::new(Orientation::Horizontal, 8);
    let path_entry = Entry::new();
    path_entry.set_text(&initial_path.to_string_lossy());
    path_entry.set_hexpand(true);
    path_entry.set_activates_default(true);
    let browse_button = Button::with_label("Browse\u{2026}");
    path_row.append(&path_entry);
    path_row.append(&browse_button);
    content.append(&path_row);

    let password = Entry::new();
    password.set_placeholder_text(Some("Master password (8 characters minimum)"));
    password.set_visibility(false);
    password.set_input_purpose(gtk::InputPurpose::Password);
    password.set_activates_default(true);
    content.append(&password);

    let confirmation = Entry::new();
    confirmation.set_placeholder_text(Some("Confirm master password"));
    confirmation.set_visibility(false);
    confirmation.set_input_purpose(gtk::InputPurpose::Password);
    confirmation.set_activates_default(true);
    content.append(&confirmation);
    password.grab_focus();

    let error_label = Label::new(None);
    error_label.set_xalign(0.0);
    error_label.add_css_class("error");
    content.append(&error_label);

    let spinner = Spinner::new();
    spinner.set_halign(gtk::Align::Center);
    spinner.set_margin_top(4);
    spinner.set_visible(false);
    content.append(&spinner);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.set_halign(gtk::Align::End);
    action_row.set_margin_top(12);
    action_row.set_margin_bottom(4);
    let cancel_button = Button::with_label("Cancel");
    let action_button = Button::with_label("Open");
    action_button.add_css_class("suggested-action");
    dialog.set_default_widget(Some(&action_button));
    action_row.append(&cancel_button);
    action_row.append(&action_button);
    content.append(&action_row);

    dialog.content_area().append(&content);

    let update_for_path = {
        let path_entry = path_entry.clone();
        let confirmation = confirmation.clone();
        let action_button = action_button.clone();
        move || {
            let path_text = path_entry.text().to_string();
            let trimmed = path_text.trim();
            let exists = !trimmed.is_empty() && std::path::Path::new(trimmed).is_file();
            confirmation.set_visible(!exists);
            action_button.set_label(if exists { "Unlock" } else { "Create" });
        }
    };
    update_for_path();

    path_entry.connect_changed({
        let update_for_path = update_for_path.clone();
        move |_| update_for_path()
    });

    let dialog_for_cancel = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        dialog_for_cancel.response(ResponseType::Cancel);
    });
    let dialog_for_action = dialog.clone();
    action_button.connect_clicked(move |_| {
        dialog_for_action.response(ResponseType::Accept);
    });
    let dialog_for_escape = dialog.clone();
    let escape_controller = gtk::EventControllerKey::new();
    escape_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            dialog_for_escape.response(ResponseType::Cancel);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    dialog.add_controller(escape_controller);

    let application_for_quit = application.clone();
    let application_for_close = application.clone();
    let application_for_response = application.clone();
    let window_for_close = window.clone();
    let path_entry_for_response = path_entry.clone();
    let password_for_response = password.clone();
    let confirmation_for_response = confirmation.clone();
    let cancel_button_for_response = cancel_button.clone();
    let action_button_for_response = action_button.clone();
    let browse_button_for_response = browse_button.clone();
    let error_label_for_response = error_label.clone();
    let spinner_for_response = spinner.clone();
    let window_to_destroy_for_response = window_to_destroy.clone();

    dialog.connect_response(move |dialog, response| {
        if response != ResponseType::Accept {
            application_for_quit.quit();
            return;
        }
        let path_text = path_entry_for_response.text().to_string();
        let trimmed = path_text.trim().to_string();
        if trimmed.is_empty() {
            error_label_for_response.set_text("Please choose a vault file");
            return;
        }
        let password_text = password_for_response.text().to_string();
        let confirmation_text = confirmation_for_response.text().to_string();
        let will_create = !std::path::Path::new(&trimmed).is_file();
        if will_create {
            if password_text != confirmation_text {
                error_label_for_response.set_text("Passwords do not match");
                return;
            }
        }
        if password_text.is_empty() {
            error_label_for_response.set_text("Master password is required");
            return;
        }

        let stored_path = std::fs::canonicalize(&trimmed).unwrap_or_else(|_| PathBuf::from(&trimmed));
        let mut new_settings = AppSettings::default();
        new_settings.last_vault_path = Some(stored_path);
        let _ = new_settings.save();

        path_entry_for_response.set_sensitive(false);
        password_for_response.set_sensitive(false);
        confirmation_for_response.set_sensitive(false);
        cancel_button_for_response.set_sensitive(false);
        action_button_for_response.set_sensitive(false);
        browse_button_for_response.set_sensitive(false);
        error_label_for_response.set_text("");
        spinner_for_response.set_visible(true);
        spinner_for_response.start();

        let (sender, receiver) = std::sync::mpsc::channel::<anyhow::Result<crate::storage::Vault>>();
        let path_for_thread = trimmed.clone();
        let password_for_thread = password_text.clone();
        let will_create_for_thread = will_create;
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<crate::storage::Vault> {
                let store = VaultStore::for_path(&path_for_thread)?;
                if will_create_for_thread {
                    store.create(&password_for_thread)
                } else {
                    store.unlock(&password_for_thread)
                }
            })();
            let _ = sender.send(result);
        });

        let dialog_for_poll = dialog.clone();
        let window_for_poll = window.clone();
        let application_for_poll = application_for_response.clone();
        let password_for_poll = password_for_response.clone();
        let confirmation_for_poll = confirmation_for_response.clone();
        let cancel_button_for_poll = cancel_button_for_response.clone();
        let action_button_for_poll = action_button_for_response.clone();
        let browse_button_for_poll = browse_button_for_response.clone();
        let error_label_for_poll = error_label_for_response.clone();
        let spinner_for_poll = spinner_for_response.clone();
        let window_to_destroy_for_poll = window_to_destroy_for_response.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            match receiver.try_recv() {
                Ok(Ok(vault)) => {
                    dialog_for_poll.destroy();
                    window_for_poll.destroy();
                    if let Some(w) = window_to_destroy_for_poll.as_ref() {
                        w.destroy();
                    }
                    show_main_window(&application_for_poll, vault);
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => {
                    spinner_for_poll.stop();
                    spinner_for_poll.set_visible(false);
                    password_for_poll.set_sensitive(true);
                    confirmation_for_poll.set_sensitive(true);
                    cancel_button_for_poll.set_sensitive(true);
                    action_button_for_poll.set_sensitive(true);
                    browse_button_for_poll.set_sensitive(true);
                    password_for_poll.set_text("");
                    password_for_poll.grab_focus();
                    error_label_for_poll.set_text(&error.to_string());
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
    });

    let window_for_browse = window_for_close.clone();
    let path_entry_for_browse = path_entry.clone();
    let update_for_browse = update_for_path.clone();
    browse_button.connect_clicked(move |_| {
        let chooser = gtk::FileChooserNative::new(
            Some("Select vault file"),
            Some(&window_for_browse),
            gtk::FileChooserAction::Open,
            Some("Select"),
            Some("Cancel"),
        );
        let current = path_entry_for_browse.text().to_string();
        if !current.trim().is_empty() {
            if let Some(parent) = std::path::Path::new(&current).parent() {
                if parent.is_dir() {
                    let folder = gtk::gio::File::for_path(parent);
                    let _ = chooser.set_current_folder(Some(&folder));
                }
            }
            let file = gtk::gio::File::for_path(&current);
            if chooser.set_file(&file).is_err() {
                if let Some(name) = std::path::Path::new(&current).file_name() {
                    chooser.set_current_name(&name.to_string_lossy());
                }
            }
        }
        let path_entry_for_response = path_entry_for_browse.clone();
        let update_for_response = update_for_browse.clone();
        chooser.connect_response(move |chooser, response| {
            if response == ResponseType::Accept {
                if let Some(file) = chooser.file() {
                    if let Some(path) = file.path() {
                        path_entry_for_response.set_text(&path.to_string_lossy());
                        update_for_response();
                    }
                }
            }
            chooser.destroy();
        });
        chooser.show();
    });

    window_for_close.connect_close_request(move |_| {
        application_for_close.quit();
        glib::Propagation::Proceed
    });
    dialog.present();
    window_for_close.present();

    if !has_saved_path {
        let auto_browse = Rc::new(Cell::new(false));
        let auto_browse_for_map = auto_browse.clone();
        let browse_for_map = browse_button.clone();
        window_for_close.connect_map(move |_| {
            if !auto_browse_for_map.get() {
                auto_browse_for_map.set(true);
                browse_for_map.emit_clicked();
            }
        });
    }
}

#[derive(Clone)]
struct DetailWidgets {
    stack: Stack,
    issuer: Label,
    account: Label,
    code: Label,
    countdown: CountdownSpinner,
    metadata: Label,
    copy: Button,
    edit: Button,
    remove: Button,
}

#[derive(Clone)]
struct CountdownSpinner {
    area: DrawingArea,
    state: Rc<RefCell<CountdownState>>,
}

struct CountdownState {
    fraction: f64,
    remaining: u64,
}

impl CountdownSpinner {
    fn new() -> Self {
        let state = Rc::new(RefCell::new(CountdownState {
            fraction: 1.0,
            remaining: 0,
        }));
        let area = DrawingArea::new();
        area.set_content_width(96);
        area.set_content_height(96);
        area.set_halign(gtk::Align::Start);
        let state_for_draw = state.clone();
        area.set_draw_func(move |area, cr, width, height| {
            let state = state_for_draw.borrow();
            let size = width.min(height) as f64;
            let line_width = (size * 0.08).max(4.0);
            let radius = (size - line_width) / 2.0;
            let cx = width as f64 / 2.0;
            let cy = height as f64 / 2.0;

            cr.set_line_width(line_width);
            cr.set_line_cap(gtk::cairo::LineCap::Round);

            let style = area.style_context();
            let color = style.color();
            let r = color.red() as f64;
            let g = color.green() as f64;
            let b = color.blue() as f64;

            cr.set_source_rgba(r, g, b, 0.2);
            cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.stroke().ok();

            let (fr, fg, fb) = countdown_color(state.remaining);
            let angle = 2.0 * std::f64::consts::PI * state.fraction.clamp(0.0, 1.0);
            let start = -std::f64::consts::PI / 2.0;
            if angle > 0.0 {
                cr.set_source_rgba(fr, fg, fb, 0.9);
                cr.arc(cx, cy, radius, start, start + angle);
                cr.stroke().ok();
            }

            let text = format!("{}s", state.remaining);
            let font_size = (size * 0.28).max(14.0);
            cr.select_font_face(
                "Sans",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            cr.set_font_size(font_size);
            let extents = cr.text_extents(&text).ok();
            if let Some(extents) = extents {
                let tx = cx - (extents.width() / 2.0 + extents.x_bearing());
                let ty = cy - (extents.height() / 2.0 + extents.y_bearing());
                cr.set_source_rgba(fr, fg, fb, 1.0);
                cr.move_to(tx, ty);
                cr.show_text(&text).ok();
            }
        });
        Self { area, state }
    }

    fn update(&self, fraction: f64, remaining: u64) {
        let mut state = self.state.borrow_mut();
        let changed = (state.fraction - fraction).abs() > f64::EPSILON || state.remaining != remaining;
        state.fraction = fraction;
        state.remaining = remaining;
        if changed {
            self.area.queue_draw();
        }
    }
}

impl std::ops::Deref for CountdownSpinner {
    type Target = DrawingArea;
    fn deref(&self) -> &DrawingArea {
        &self.area
    }
}

fn countdown_color(remaining: u64) -> (f64, f64, f64) {
    if remaining <= 5 {
        (0.752, 0.110, 0.157)
    } else if remaining <= 10 {
        (0.898, 0.647, 0.039)
    } else {
        (0.180, 0.761, 0.494)
    }
}

#[derive(Clone)]
struct RenderContext {
    vault: Rc<RefCell<Vault>>,
    list_box: ListBox,
    row_labels: Rc<RefCell<HashMap<Uuid, Label>>>,
    detail: DetailWidgets,
    selected: Rc<Cell<Option<Uuid>>>,
    filter: Rc<RefCell<String>>,
    toast: Rc<RefCell<Option<Label>>>,
    toast_overlay: Rc<RefCell<Option<GtkBox>>>,
    last_counted_period: Rc<RefCell<HashMap<Uuid, u64>>>,
    vault_dirty: Rc<Cell<bool>>,
}

fn show_main_window(application: &Application, vault: Vault) {
    let settings = AppSettings::load().unwrap_or_default();
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Notp")
        .default_width(900)
        .default_height(600)
        .build();

    apply_theme(&settings.theme);

    let vault = Rc::new(RefCell::new(vault));
    let selected = Rc::new(Cell::new(None::<Uuid>));
    let row_labels = Rc::new(RefCell::new(HashMap::new()));
    let filter = Rc::new(RefCell::new(String::new()));
    let toast = Rc::new(RefCell::new(None::<Label>));
    let toast_overlay = Rc::new(RefCell::new(None::<GtkBox>));

    let header = HeaderBar::new();
    let header_box = GtkBox::new(Orientation::Horizontal, 6);
    header_box.set_margin_start(8);
    header_box.set_margin_end(8);
    let add_button = Button::with_label("Add entry");

    let import_popover = Popover::new();
    let import_options = GtkBox::new(Orientation::Vertical, 0);
    import_options.set_margin_top(4);
    import_options.set_margin_bottom(4);
    import_options.set_margin_start(4);
    import_options.set_margin_end(4);
    let import_image_option = Button::with_label("Import from image\u{2026}");
    import_image_option.set_has_frame(false);
    import_image_option.set_halign(gtk::Align::Fill);
    let import_camera_option = Button::with_label("Scan with camera\u{2026}");
    import_camera_option.set_has_frame(false);
    import_camera_option.set_halign(gtk::Align::Fill);
    import_options.append(&import_image_option);
    import_options.append(&import_camera_option);
    import_popover.set_child(Some(&import_options));

    let import_menu_button = MenuButton::new();
    import_menu_button.set_icon_name("document-open-symbolic");
    import_menu_button.set_popover(Some(&import_popover));
    import_menu_button.set_tooltip_text(Some("Import an entry from an image or camera"));
    import_menu_button.set_valign(gtk::Align::Center);

    let lock_button = Button::from_icon_name("system-lock-screen-symbolic");
    lock_button.set_tooltip_text(Some("Lock the vault (Ctrl+L)"));
    lock_button.set_valign(gtk::Align::Center);

    let menu_popover = Popover::new();
    let menu_box = GtkBox::new(Orientation::Vertical, 0);
    menu_box.set_margin_top(4);
    menu_box.set_margin_bottom(4);
    menu_box.set_margin_start(4);
    menu_box.set_margin_end(4);
    let preferences_button = menu_button("Preferences\u{2026}");
    let change_password_button = menu_button("Change master password\u{2026}");
    let about_button = menu_button("About\u{2026}");
    menu_box.append(&preferences_button);
    menu_box.append(&change_password_button);
    menu_box.append(&Separator::new(Orientation::Horizontal));
    menu_box.append(&about_button);
    menu_popover.set_child(Some(&menu_box));

    let menu_button = MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_popover(Some(&menu_popover));
    menu_button.set_tooltip_text(Some("Application menu"));
    menu_button.set_valign(gtk::Align::Center);

    header_box.append(&add_button);
    header_box.append(&import_menu_button);
    header_box.append(&lock_button);
    header.pack_start(&header_box);
    header.pack_end(&menu_button);
    window.set_titlebar(Some(&header));

    let list_scroller = ScrolledWindow::new();
    list_scroller.set_min_content_width(280);
    list_scroller.set_hexpand(true);
    list_scroller.set_vexpand(true);
    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);
    list_box.set_activate_on_single_click(false);
    list_scroller.set_child(Some(&list_box));

    let search_entry = Entry::new();
    search_entry.set_placeholder_text(Some("Search entries\u{2026}"));
    search_entry.set_margin_start(12);
    search_entry.set_margin_end(12);
    search_entry.set_margin_top(8);
    search_entry.set_margin_bottom(4);

    let detail_stack = Stack::new();
    detail_stack.set_hexpand(true);
    detail_stack.set_vexpand(true);
    let empty_state = GtkBox::new(Orientation::Vertical, 12);
    empty_state.set_valign(gtk::Align::Center);
    empty_state.set_halign(gtk::Align::Center);
    let empty_title = Label::new(Some("No entry selected"));
    empty_title.add_css_class("title-2");
    empty_state.append(&empty_title);
    let empty_text = Label::new(Some("Add an entry to start generating codes."));
    empty_text.set_wrap(true);
    empty_state.append(&empty_text);
    detail_stack.add_named(&empty_state, Some("empty"));

    let detail_content = GtkBox::new(Orientation::Vertical, 14);
    detail_content.set_margin_start(24);
    detail_content.set_margin_end(24);
    detail_content.set_margin_top(24);
    detail_content.set_margin_bottom(24);
    let detail_issuer = Label::new(None);
    detail_issuer.add_css_class("title-1");
    detail_issuer.set_xalign(0.0);
    detail_content.append(&detail_issuer);
    let detail_account = Label::new(None);
    detail_account.set_xalign(0.0);
    detail_content.append(&detail_account);
    let detail_code = Label::new(Some("------"));
    detail_code.add_css_class("title-1");
    detail_code.set_xalign(0.0);
    detail_code.set_selectable(true);
    detail_content.append(&detail_code);
    let detail_countdown = CountdownSpinner::new();
    detail_content.append(&*detail_countdown);
    let detail_metadata = Label::new(None);
    detail_metadata.set_xalign(0.0);
    detail_metadata.add_css_class("dim-label");
    detail_metadata.set_wrap(true);
    detail_content.append(&detail_metadata);

    let action_box = GtkBox::new(Orientation::Horizontal, 8);
    let copy_button = Button::with_label("Copy code");
    copy_button.set_sensitive(false);
    let edit_button = Button::with_label("Edit");
    edit_button.set_sensitive(false);
    let remove_button = Button::with_label("Delete");
    remove_button.set_sensitive(false);
    action_box.append(&copy_button);
    action_box.append(&edit_button);
    action_box.append(&remove_button);
    detail_content.append(&action_box);
    detail_stack.add_named(&detail_content, Some("content"));
    detail_stack.set_visible_child_name("empty");

    let list_box_container = GtkBox::new(Orientation::Vertical, 0);
    list_box_container.append(&search_entry);
    list_box_container.append(&list_scroller);

    let paned = Paned::new(Orientation::Horizontal);
    paned.set_vexpand(true);
    paned.set_resize_start_child(true);
    paned.set_shrink_start_child(false);
    paned.set_resize_end_child(true);
    paned.set_shrink_end_child(false);
    paned.set_start_child(Some(&list_box_container));
    paned.set_end_child(Some(&detail_stack));
    paned.set_position(300);

    let main_box = GtkBox::new(Orientation::Vertical, 0);
    main_box.append(&paned);

    let toast_box = GtkBox::new(Orientation::Horizontal, 0);
    toast_box.add_css_class("notp-toast");
    toast_box.set_halign(gtk::Align::End);
    toast_box.set_valign(gtk::Align::End);
    toast_box.set_margin_end(24);
    toast_box.set_margin_bottom(24);
    toast_box.set_visible(false);
    let toast_label = Label::new(None);
    toast_label.set_xalign(0.5);
    toast_box.append(&toast_label);

    let overlay = Overlay::new();
    overlay.set_child(Some(&main_box));
    overlay.add_overlay(&toast_box);
    window.set_child(Some(&overlay));

    let detail = DetailWidgets {
        stack: detail_stack,
        issuer: detail_issuer,
        account: detail_account,
        code: detail_code,
        countdown: detail_countdown,
        metadata: detail_metadata,
        copy: copy_button,
        edit: edit_button,
        remove: remove_button,
    };
    let context = RenderContext {
        vault: vault.clone(),
        list_box: list_box.clone(),
        row_labels: row_labels.clone(),
        detail: DetailWidgets {
            stack: detail.stack.clone(),
            issuer: detail.issuer.clone(),
            account: detail.account.clone(),
            code: detail.code.clone(),
            countdown: detail.countdown.clone(),
            metadata: detail.metadata.clone(),
            copy: detail.copy.clone(),
            edit: detail.edit.clone(),
            remove: detail.remove.clone(),
        },
        selected: selected.clone(),
        filter: filter.clone(),
        toast: toast.clone(),
        toast_overlay: toast_overlay.clone(),
        last_counted_period: Rc::new(RefCell::new(HashMap::new())),
        vault_dirty: Rc::new(Cell::new(false)),
    };
    *toast.borrow_mut() = Some(toast_label);
    *toast_overlay.borrow_mut() = Some(toast_box);

    list_box.connect_row_selected({
        let context = context.clone();
        move |_, row| {
            context
                .selected
                .set(row.and_then(|row| Uuid::parse_str(row.widget_name().as_str()).ok()));
            show_selected(&context);
            refresh_codes(&context);
        }
    });

    search_entry.connect_changed({
        let context = context.clone();
        move |entry| {
            *context.filter.borrow_mut() = entry.text().to_string();
            render_accounts(&context);
        }
    });

    let context_for_add = context.clone();
    let window_for_add = window.clone();
    add_button.connect_clicked(move |_| {
        let context_for_result = context_for_add.clone();
        let window_for_result = window_for_add.clone();
        add_account_dialog(&window_for_add, None, move |account| {
            let window_for_error = window_for_result.clone();
            let Some(account) = account else {
                return;
            };
            let id = match context_for_result
                .vault
                .borrow_mut()
                .data_mut()
                .add_account(account)
            {
                Ok(id) => id,
                Err(error) => {
                    show_error(&window_for_error, "Invalid entry", &error.to_string());
                    return;
                }
            };
            let save_result = context_for_result.vault.borrow().save();
            let save_result = match save_result {
                Ok(()) => Ok(()),
                Err(error) => {
                    context_for_result
                        .vault
                        .borrow_mut()
                        .data_mut()
                        .remove_account(id);
                    Err(error)
                }
            };
            if let Err(error) = save_result {
                show_error(
                    &window_for_error,
                    "Unable to save the vault",
                    &error.to_string(),
                );
            } else {
                context_for_result.selected.set(Some(id));
                render_accounts(&context_for_result);
            }
        });
    });

    let window_for_import = window.clone();
    let context_for_import = context.clone();
    let window_for_camera = window.clone();
    let context_for_camera = context.clone();
    let popover_for_camera = import_popover.clone();
    import_camera_option.connect_clicked(move |_| {
        popover_for_camera.popdown();
        let (sender, receiver) =
            std::sync::mpsc::channel::<anyhow::Result<crate::qr_import::OtpParams>>();
        if let Err(error) = crate::camera::start_scan(move |result| {
            let _ = sender.send(result);
        }) {
            show_error(&window_for_camera, "Camera unavailable", &error.to_string());
            return;
        }
        let window_for_dialog = window_for_camera.clone();
        let context_for_dialog = context_for_camera.clone();
        glib::timeout_add_seconds_local(1, move || match receiver.try_recv() {
            Ok(Ok(params)) => {
                let window_for_callback = window_for_dialog.clone();
                let context_for_callback = context_for_dialog.clone();
                add_account_dialog(&window_for_dialog, Some(params), move |account| {
                    let window_for_error = window_for_callback.clone();
                    let Some(account) = account else {
                        return;
                    };
                    let id = match context_for_callback
                        .vault
                        .borrow_mut()
                        .data_mut()
                        .add_account(account)
                    {
                        Ok(id) => id,
                        Err(error) => {
                            show_error(&window_for_error, "Invalid entry", &error.to_string());
                            return;
                        }
                    };
                    let save_result = context_for_callback.vault.borrow().save();
                    let save_result = match save_result {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            context_for_callback
                                .vault
                                .borrow_mut()
                                .data_mut()
                                .remove_account(id);
                            Err(error)
                        }
                    };
                    if let Err(error) = save_result {
                        show_error(
                            &window_for_error,
                            "Unable to save the vault",
                            &error.to_string(),
                        );
                    } else {
                        context_for_callback.selected.set(Some(id));
                        render_accounts(&context_for_callback);
                    }
                });
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                show_error(&window_for_dialog, "Camera scan failed", &error.to_string());
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        });
    });
    let popover_for_import = import_popover.clone();
    import_image_option.connect_clicked(move |_| {
        popover_for_import.popdown();
        let window_for_error = window_for_import.clone();
        let chooser = gtk::FileChooserNative::new(
            Some("Import a QR code"),
            Some(&window_for_import),
            gtk::FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Images"));
        filter.add_pixbuf_formats();
        chooser.add_filter(&filter);
        let window_for_dialog = window_for_import.clone();
        let context_for_dialog = context_for_import.clone();
        chooser.connect_response(move |chooser, response| {
            if response == ResponseType::Accept {
                if let Some(file) = chooser.file() {
                    if let Some(path) = file.path() {
                        match crate::qr_import::decode_qr_from_path(&path) {
                            Ok(params) if params.len() == 1 => {
                                let context_for_result = context_for_dialog.clone();
                                let window_for_result = window_for_dialog.clone();
                                let params = params.into_iter().next().unwrap();
                                add_account_dialog(
                                    &window_for_dialog,
                                    Some(params),
                                    move |account| {
                                        let window_for_error = window_for_result.clone();
                                        let Some(account) = account else {
                                            return;
                                        };
                                        let id = match context_for_result
                                            .vault
                                            .borrow_mut()
                                            .data_mut()
                                            .add_account(account)
                                        {
                                            Ok(id) => id,
                                            Err(error) => {
                                                show_error(
                                                    &window_for_error,
                                                    "Invalid entry",
                                                    &error.to_string(),
                                                );
                                                return;
                                            }
                                        };
                                        let save_result = context_for_result.vault.borrow().save();
                                        let save_result = match save_result {
                                            Ok(()) => Ok(()),
                                            Err(error) => {
                                                context_for_result
                                                    .vault
                                                    .borrow_mut()
                                                    .data_mut()
                                                    .remove_account(id);
                                                Err(error)
                                            }
                                        };
                                        if let Err(error) = save_result {
                                            show_error(
                                                &window_for_error,
                                                "Unable to save the vault",
                                                &error.to_string(),
                                            );
                                        } else {
                                            context_for_result.selected.set(Some(id));
                                            render_accounts(&context_for_result);
                                        }
                                    },
                                );
                            }
                            Ok(params) => {
                                let context_for_result = context_for_dialog.clone();
                                let window_for_result = window_for_dialog.clone();
                                let count = params.len();
                                confirm(
                                    &window_for_dialog,
                                    "Confirm bulk import",
                                    &format!(
                                        "This QR code contains {count} entries. Import them all?"
                                    ),
                                    move || {
                                        import_entries(
                                            &window_for_result,
                                            &context_for_result,
                                            params,
                                        );
                                    },
                                );
                            }
                            Err(error) => {
                                show_error(
                                    &window_for_error,
                                    "Unable to import QR",
                                    &error.to_string(),
                                );
                            }
                        }
                    } else {
                        show_error(
                            &window_for_error,
                            "Unable to import QR",
                            "Selected file has no local path",
                        );
                    }
                }
            }
        });
        chooser.show();
    });

    let context_for_copy = context.clone();
    let window_for_copy = window.clone();
    detail.copy.connect_clicked(move |_| {
        copy_current_code(&context_for_copy, &window_for_copy, ClipboardTarget::Code);
    });

    let context_for_edit = context.clone();
    let window_for_edit = window.clone();
    detail.edit.connect_clicked(move |_| {
        let Some(id) = context_for_edit.selected.get() else {
            return;
        };
        edit_selected(&context_for_edit, &window_for_edit, id);
    });

    let context_for_remove = context.clone();
    let window_for_remove = window.clone();
    detail.remove.connect_clicked(move |_| {
        let Some(id) = context_for_remove.selected.get() else {
            return;
        };
        let account = {
            let vault = context_for_remove.vault.borrow();
            vault
                .data()
                .account(id)
                .map(|account| (account.issuer.clone(), account.name.clone()))
        };
        if let Some((issuer, name)) = account {
            let message = format!("Delete the entry « {} » ({}) ?", name, issuer);
            let context_for_confirmation = context_for_remove.clone();
            let window_for_confirmation = window_for_remove.clone();
            confirm(&window_for_remove, "Delete entry", &message, move || {
                let result = context_for_confirmation
                    .vault
                    .borrow_mut()
                    .remove_account(id);
                if let Err(error) = result {
                    show_error(
                        &window_for_confirmation,
                        "Unable to save the vault",
                        &error.to_string(),
                    );
                } else {
                    context_for_confirmation.selected.set(None);
                    render_accounts(&context_for_confirmation);
                }
            });
        }
    });

    render_accounts(&context);
    let context_for_tick = context.clone();
    glib::timeout_add_seconds_local(1, move || {
        refresh_codes(&context_for_tick);
        poll_clipboard_auto_clear();
        glib::ControlFlow::Continue
    });

    let application = application.clone();
    let application_for_close = application.clone();
    window.connect_close_request(move |_| {
        application_for_close.quit();
        glib::Propagation::Proceed
    });

    let lock_action: Rc<dyn Fn()> = {
        let application = application.clone();
        let window = window.clone();
        Rc::new(move || {
            window.set_visible(false);
            show_load_window(&application, Some(window.clone()));
        })
    };

    lock_button.connect_clicked({
        let lock_action = lock_action.clone();
        move |_| lock_action()
    });

    let has_been_active: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let last_inactive: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let window_for_focus = window.clone();
    let has_been_active_for_focus = has_been_active.clone();
    let last_inactive_for_focus = last_inactive.clone();
    window_for_focus.connect_is_active_notify(move |window| {
        if window.is_active() {
            has_been_active_for_focus.set(true);
            last_inactive_for_focus.set(None);
        } else if has_been_active_for_focus.get() {
            last_inactive_for_focus.set(Some(Instant::now()));
        }
    });

    let has_been_active_for_timer = has_been_active.clone();
    let last_inactive_for_timer = last_inactive.clone();
    let window_for_timer = window.clone();
    let lock_action_for_timer = lock_action.clone();
    let settings_for_timer = Rc::new(Cell::new(settings.auto_lock_seconds));
    let settings_for_timer_clone = settings_for_timer.clone();
    glib::timeout_add_seconds_local(1, move || {
        if has_been_active_for_timer.get() && !window_for_timer.is_active() {
            let now = Instant::now();
            let last = last_inactive_for_timer
                .get()
                .unwrap_or_else(|| {
                    last_inactive_for_timer.set(Some(now));
                    now
                });
            if now.duration_since(last).as_secs() >= settings_for_timer_clone.get() {
                lock_action_for_timer();
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    });

    let popover_for_menu = menu_popover.clone();
    let settings_for_prefs = settings_for_timer.clone();
    let window_for_prefs = window.clone();
    let context_for_prefs = context.clone();
    preferences_button.connect_clicked(move |_| {
        popover_for_menu.popdown();
        let settings_for_apply = settings_for_prefs.clone();
        let window_for_apply = window_for_prefs.clone();
        let context_for_apply = context_for_prefs.clone();
        preferences_dialog(
            &window_for_prefs,
            AppSettings::load().unwrap_or_default(),
            move |new_settings| {
                settings_for_apply.set(new_settings.auto_lock_seconds);
                apply_theme(&new_settings.theme);
                if let Err(error) = new_settings.save() {
                    show_error(
                        &window_for_apply,
                        "Unable to save preferences",
                        &error.to_string(),
                    );
                }
                let _ = context_for_apply;
            },
        );
    });

    let popover_for_password = menu_popover.clone();
    let window_for_password = window.clone();
    let context_for_password = context.clone();
    change_password_button.connect_clicked(move |_| {
        popover_for_password.popdown();
        let window_for_apply = window_for_password.clone();
        let context_for_change = context_for_password.clone();
        change_password_dialog(&window_for_password, move |old, new_password| {
            let result = context_for_change
                .vault
                .borrow_mut()
                .change_password(old, new_password);
            if let Err(error) = result {
                show_error(
                    &window_for_apply,
                    "Unable to change password",
                    &error.to_string(),
                );
            }
        });
    });

    let popover_for_about = menu_popover.clone();
    let window_for_about = window.clone();
    about_button.connect_clicked(move |_| {
        popover_for_about.popdown();
        show_about_dialog(&window_for_about);
    });

    // --- Keyboard shortcuts -------------------------------------------------
    let key_controller = EventControllerKey::new();
    let context_for_keys = context.clone();
    let list_box_for_keys = list_box.clone();
    let search_for_keys = search_entry.clone();
    let window_for_keys = window.clone();
    let add_button_for_keys = add_button.clone();
    key_controller.connect_key_pressed(move |_, key, _keycode, state| {
        let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        if ctrl && key == gtk::gdk::Key::n {
            add_button_for_keys.emit_clicked();
            return glib::Propagation::Stop;
        }
        if ctrl && !shift && key == gtk::gdk::Key::e {
            if let Some(id) = context_for_keys.selected.get() {
                edit_selected(&context_for_keys, &window_for_keys, id);
            }
            return glib::Propagation::Stop;
        }
        if ctrl && !shift && key == gtk::gdk::Key::c {
            copy_current_code(&context_for_keys, &window_for_keys, ClipboardTarget::Code);
            return glib::Propagation::Stop;
        }
        if ctrl && shift && key == gtk::gdk::Key::C {
            copy_current_code(&context_for_keys, &window_for_keys, ClipboardTarget::Secret);
            return glib::Propagation::Stop;
        }
        if ctrl && key == gtk::gdk::Key::l {
            lock_action();
            return glib::Propagation::Stop;
        }
        if ctrl && key == gtk::gdk::Key::f {
            search_for_keys.grab_focus();
            return glib::Propagation::Stop;
        }
        if key == gtk::gdk::Key::Delete || key == gtk::gdk::Key::KP_Delete {
            if let Some(id) = context_for_keys.selected.get() {
                request_delete(&context_for_keys, &window_for_keys, id);
            }
            return glib::Propagation::Stop;
        }
        if !ctrl && !shift {
            match key {
                gtk::gdk::Key::Home => {
                    select_first_or_last(&list_box_for_keys, true);
                    return glib::Propagation::Stop;
                }
                gtk::gdk::Key::End => {
                    select_first_or_last(&list_box_for_keys, false);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.present();
}

fn attach_drag_handlers(context: &RenderContext, row: &ListBoxRow, id: Uuid) {
    let drag_source = DragSource::new();
    drag_source.set_actions(DragAction::MOVE);
    let id_string = id.to_string();
    let content = ContentProvider::for_value(&id_string.to_value());
    drag_source.set_content(Some(&content));
    let row_for_icon = row.clone();
    drag_source.connect_drag_begin(move |source, _drag| {
        let paintable = WidgetPaintable::new(Some(&row_for_icon));
        source.set_icon(Some(&paintable), 0, 0);
        row_for_icon.add_css_class("dragging");
    });
    let row_for_source_end = row.clone();
    drag_source.connect_drag_end(move |_source, _drag, _delete| {
        row_for_source_end.remove_css_class("dragging");
    });
    row.add_controller(drag_source);

    let drop_target = DropTarget::new(glib::Type::STRING, DragAction::MOVE);
    let context_for_drop = context.clone();
    let context_for_enter = context.clone();
    drop_target.connect_enter(move |target, _x, y| {
        if let Some(widget) = target.widget() {
            if let Ok(row) = widget.downcast::<ListBoxRow>() {
                let height = row.height() as f64;
                let after = height > 0.0 && y > height / 2.0;
                if after {
                    row.add_css_class("drop-after");
                    row.remove_css_class("drop-before");
                } else {
                    row.add_css_class("drop-before");
                    row.remove_css_class("drop-after");
                }
                let _ = &context_for_enter;
            }
        }
        DragAction::MOVE
    });
    drop_target.connect_leave(move |target| {
        if let Some(widget) = target.widget() {
            if let Ok(row) = widget.downcast::<ListBoxRow>() {
                row.remove_css_class("drop-before");
                row.remove_css_class("drop-after");
            }
        }
    });

    drop_target.connect_local("drop", false, move |values| {
        let drop_target_obj = match values[0].get::<DropTarget>() {
            Ok(t) => t,
            Err(_) => return Some(false.to_value()),
        };
        let _drop = values[1].get::<gtk::gdk::Drop>().ok();
        let y = values[3].get::<f64>().unwrap_or(0.0);
        let widget = match drop_target_obj.widget() {
            Some(w) => w,
            None => return Some(false.to_value()),
        };
        let row = match widget.downcast::<ListBoxRow>() {
            Ok(r) => r,
            Err(_) => return Some(false.to_value()),
        };
        let value = match drop_target_obj.value() {
            Some(v) => v,
            None => return Some(false.to_value()),
        };
        let source_id = match value.get::<String>() {
            Ok(text) => match Uuid::parse_str(&text) {
                Ok(id) => id,
                Err(_) => return Some(false.to_value()),
            },
            Err(_) => return Some(false.to_value()),
        };
        let total = {
            let vault = context_for_drop.vault.borrow();
            vault.data().accounts.len()
        };
        let target_index = row.index() as usize;
        let height = row.height() as f64;
        let insert_after = height > 0.0 && y > height / 2.0;
        let desired = if insert_after {
            target_index + 1
        } else {
            target_index
        };
        let new_position = desired.min(total);
        let result = context_for_drop
            .vault
            .borrow_mut()
            .reorder_account(source_id, new_position);
        match result {
            Ok(Some(_)) => {
                context_for_drop.selected.set(Some(source_id));
                render_accounts(&context_for_drop);
                Some(true.to_value())
            }
            Ok(None) | Err(_) => Some(false.to_value()),
        }
    });

    row.add_controller(drop_target);
}

fn render_accounts(context: &RenderContext) {
    while let Some(child) = context.list_box.first_child() {
        context.list_box.remove(&child);
    }
    context.row_labels.borrow_mut().clear();

    let filter = context.filter.borrow().clone();
    let needle = filter.trim().to_lowercase();

    let accounts = {
        let vault = context.vault.borrow();
        vault
            .data()
            .accounts
            .iter()
            .filter(|account| {
                needle.is_empty()
                    || account.issuer.to_lowercase().contains(&needle)
                    || account.name.to_lowercase().contains(&needle)
            })
            .map(|account| (account.id, account.issuer.clone(), account.name.clone()))
            .collect::<Vec<_>>()
    };

    let selected = context.selected.get();
    let mut first_id = None;
    for (id, issuer, name) in accounts {
        let row = ListBoxRow::new();
        row.set_widget_name(&id.to_string());
        let row_box = GtkBox::new(Orientation::Horizontal, 12);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);
        row_box.set_margin_top(10);
        row_box.set_margin_bottom(10);
        let title_box = GtkBox::new(Orientation::Vertical, 3);
        let title = Label::new(Some(&issuer));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_hexpand(true);
        let account_name = Label::new(Some(&name));
        account_name.set_xalign(0.0);
        account_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_box.append(&title);
        title_box.append(&account_name);
        let code_label = Label::new(Some("------"));
        code_label.add_css_class("monospace");
        row_box.append(&title_box);
        row_box.append(&code_label);
        row.set_child(Some(&row_box));
        attach_drag_handlers(&context, &row, id);
        context.list_box.append(&row);
        context.row_labels.borrow_mut().insert(id, code_label);
        if first_id.is_none() {
            first_id = Some(id);
        }
    }

    let selected = selected
        .filter(|_| context.list_box.row_at_index(0).map_or(false, |_| true))
        .or(first_id);
    context.selected.set(selected);
    if let Some(id) = selected {
        let mut position = 0;
        while let Some(row) = context.list_box.row_at_index(position) {
            if row.widget_name() == id.to_string() {
                context.list_box.select_row(Some(&row));
                break;
            }
            position += 1;
        }
    }
    show_selected(context);
    refresh_codes(context);
}

fn show_selected(context: &RenderContext) {
    let Some(id) = context.selected.get() else {
        context.detail.stack.set_visible_child_name("empty");
        context.detail.copy.set_sensitive(false);
        context.detail.edit.set_sensitive(false);
        context.detail.remove.set_sensitive(false);
        context.detail.metadata.set_text("");
        return;
    };
    let vault = context.vault.borrow();
    let Some(account) = vault.data().account(id) else {
        drop(vault);
        context.selected.set(None);
        context.detail.stack.set_visible_child_name("empty");
        context.detail.copy.set_sensitive(false);
        context.detail.edit.set_sensitive(false);
        context.detail.remove.set_sensitive(false);
        context.detail.metadata.set_text("");
        return;
    };
    context.detail.issuer.set_text(&account.issuer);
    context.detail.account.set_text(&account.name);
    context.detail.metadata.set_text(&format_metadata(account.added_at, account.last_used_at, account.use_count));
    context.detail.stack.set_visible_child_name("content");
    context.detail.copy.set_sensitive(true);
    context.detail.edit.set_sensitive(true);
    context.detail.remove.set_sensitive(true);
}

fn format_metadata(added_at: u64, last_used_at: Option<u64>, use_count: u64) -> String {
    let mut parts = Vec::new();
    if added_at > 0 {
        parts.push(format!("Added {}", format_relative(added_at, current_timestamp())));
    }
    match last_used_at {
        Some(timestamp) => parts.push(format!(
            "Last used {} — {} {}",
            format_relative(timestamp, current_timestamp()),
            use_count,
            if use_count <= 1 { "time" } else { "times" }
        )),
        None => parts.push(format!("Not used yet — {} use{}", use_count, if use_count <= 1 { "" } else { "s" })),
    }
    parts.join("\n")
}

fn format_relative(then: u64, now: u64) -> String {
    if then > now {
        return "in the future".to_string();
    }
    let delta = now - then;
    if delta < 60 {
        format!("{} seconds ago", delta)
    } else if delta < 3_600 {
        format!("{} minutes ago", delta / 60)
    } else if delta < 86_400 {
        format!("{} hours ago", delta / 3_600)
    } else if delta < 86_400 * 30 {
        format!("{} days ago", delta / 86_400)
    } else if delta < 86_400 * 365 {
        format!("{} months ago", delta / (86_400 * 30))
    } else {
        format!("{} years ago", delta / (86_400 * 365))
    }
}

fn refresh_codes(context: &RenderContext) {
    let timestamp = current_timestamp();
    let labels = context.row_labels.borrow();
    let vault = context.vault.borrow();
    for account in &vault.data().accounts {
        let code = generate_code(
            account.secret(),
            timestamp,
            account.digits,
            account.period,
            account.algorithm,
        )
        .unwrap_or_else(|_| "------".to_string());
        if let Some(label) = labels.get(&account.id) {
            label.set_text(&code);
        }
    }
    if let Some(id) = context.selected.get() {
        if let Some(account) = vault.data().account(id) {
            let code = generate_code(
                account.secret(),
                timestamp,
                account.digits,
                account.period,
                account.algorithm,
            )
            .unwrap_or_else(|_| "------".to_string());
            context.detail.code.set_text(&code);
            context.detail.metadata.set_text(&format_metadata(
                account.added_at,
                account.last_used_at,
                account.use_count,
            ));
            let remaining = remaining_seconds(timestamp, account.period);
            let period = account.period;
            let fraction = if period > 0 {
                remaining as f64 / period as f64
            } else {
                0.0
            };
            context.detail.countdown.update(fraction, remaining);

            // Bump the usage counter at most once per TOTP period when the
            // code is shown in the detail view, so the metric does not race
            // with the per-second refresh loop.
            let current_period = timestamp / u64::from(period.max(1));
            let mut counted = context.last_counted_period.borrow_mut();
            let already_counted = counted.get(&id).copied() == Some(current_period);
            if !already_counted {
                counted.insert(id, current_period);
                drop(counted);
                drop(vault);
                let mut vault_mut = context.vault.borrow_mut();
                if let Some(slot) = vault_mut
                    .data_mut()
                    .accounts
                    .iter_mut()
                    .find(|slot| slot.id == id)
                {
                    slot.record_use();
                    context.vault_dirty.set(true);
                }
            }
        }
    }
    // Persist usage bumps after refreshing the visible state so we save at
    // most once per tick rather than after every interaction.
    if context.vault_dirty.get() {
        context.vault_dirty.set(false);
        if let Err(error) = context.vault.borrow().save() {
            eprintln!("notp: failed to persist usage metadata: {error}");
        }
    }
}

fn add_account_dialog<F>(parent: &ApplicationWindow, prefill: Option<OtpParams>, on_result: F)
where
    F: FnOnce(Option<Account>) + 'static,
{
    let parent_for_error = parent.clone();
    let dialog = Dialog::with_buttons(
        Some("Add an entry"),
        Some(parent),
        DialogFlags::MODAL,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Add", ResponseType::Accept),
        ],
    );
    dialog.set_default_size(520, 360);
    if let Some(button) = dialog.widget_for_response(ResponseType::Accept) {
        button.add_css_class("suggested-action");
        dialog.set_default_widget(Some(&button));
    }

    let grid = Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(10);
    grid.set_margin_start(20);
    grid.set_margin_end(20);
    grid.set_margin_top(14);
    grid.set_margin_bottom(14);
    let issuer_entry = Entry::new();
    issuer_entry.set_placeholder_text(Some("Google"));
    let account_entry = Entry::new();
    account_entry.set_placeholder_text(Some("user@example.com"));
    let secret_entry = Entry::new();
    secret_entry.set_placeholder_text(Some("Base32 secret"));
    secret_entry.set_activates_default(true);
    let digits = ComboBoxText::new();
    digits.append(Some("6"), "6 digits");
    digits.append(Some("8"), "8 digits");
    digits.set_active(Some(0));
    let algorithm = ComboBoxText::new();
    algorithm.append(Some("sha1"), Algorithm::Sha1.label());
    algorithm.append(Some("sha256"), Algorithm::Sha256.label());
    algorithm.append(Some("sha512"), Algorithm::Sha512.label());
    algorithm.set_active(Some(0));
    let period_adjustment = Adjustment::new(30.0, 1.0, 3600.0, 1.0, 10.0, 0.0);
    let period = SpinButton::new(Some(&period_adjustment), 1.0, 0);

    if let Some(prefill) = prefill.as_ref() {
        issuer_entry.set_text(&prefill.issuer);
        account_entry.set_text(&prefill.label);
        secret_entry.set_text(&prefill.secret);
        digits.set_active(Some(if prefill.digits == 8 { 1 } else { 0 }));
        algorithm.set_active_id(Some(match prefill.algorithm {
            Algorithm::Sha256 => "sha256",
            Algorithm::Sha512 => "sha512",
            _ => "sha1",
        }));
        period.set_value(prefill.period as f64);
    }
    issuer_entry.set_activates_default(true);
    account_entry.set_activates_default(true);
    secret_entry.set_activates_default(true);

    let mut row = 0;
    add_field(&grid, &Label::new(Some("Issuer")), &issuer_entry, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Account")), &account_entry, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Secret")), &secret_entry, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Digits")), &digits, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Algorithm")), &algorithm, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Period (seconds)")), &period, row);
    dialog.content_area().append(&grid);

    dialog.run_async(move |dialog, response| {
        let result = if response == ResponseType::Accept {
            let algorithm = match algorithm.active_id().map(|id| id.as_str().to_string()) {
                Some(value) if value == "sha256" => Algorithm::Sha256,
                Some(value) if value == "sha512" => Algorithm::Sha512,
                _ => Algorithm::Sha1,
            };
            let digits = match digits.active() {
                Some(1) => 8,
                _ => 6,
            };
            match Account::new(
                issuer_entry.text().to_string(),
                account_entry.text().to_string(),
                secret_entry.text().to_string(),
                digits,
                period.value() as u32,
                algorithm,
            ) {
                Ok(account) => Some(account),
                Err(error) => {
                    show_error(&parent_for_error, "Invalid entry", &error.to_string());
                    None
                }
            }
        } else {
            None
        };
        dialog.close();
        on_result(result);
    });
}

fn add_field<T: IsA<gtk::Widget>>(grid: &Grid, label: &Label, field: &T, row: i32) {
    grid.attach(label, 0, row, 1, 1);
    grid.attach(field, 1, row, 1, 1);
}

/// Build a frameless, full-width menu item with its label left-aligned.
/// `Button::with_label` defaults to centered text, which looks wrong inside
/// a popover; reaching for the internal Label and pinning its `xalign` is
/// the documented GTK4 way to fix that.
fn menu_button(text: &str) -> Button {
    let button = Button::with_label(text);
    button.set_has_frame(false);
    button.set_halign(gtk::Align::Fill);
    if let Some(child) = button.child() {
        if let Ok(label) = child.downcast::<Label>() {
            label.set_xalign(0.0);
            label.set_margin_start(6);
            label.set_margin_end(6);
            label.set_margin_top(4);
            label.set_margin_bottom(4);
        }
    }
    button
}

fn confirm<F>(parent: &ApplicationWindow, title: &str, message: &str, on_confirm: F)
where
    F: FnOnce() + 'static,
{
    let dialog = MessageDialog::builder()
        .buttons(ButtonsType::OkCancel)
        .text(message)
        .secondary_text(title)
        .build();
    dialog.set_transient_for(Some(parent));
    dialog.run_async(move |dialog, response| {
        let confirmed = response == ResponseType::Ok;
        dialog.close();
        if confirmed {
            on_confirm();
        }
    });
}

fn import_entries(
    parent: &ApplicationWindow,
    context: &RenderContext,
    entries: Vec<crate::qr_import::OtpParams>,
) {
    let mut accounts = Vec::with_capacity(entries.len());
    for entry in entries {
        match crate::storage::Account::new(
            entry.issuer,
            entry.label,
            entry.secret,
            entry.digits,
            entry.period,
            entry.algorithm,
        ) {
            Ok(account) => accounts.push(account),
            Err(error) => {
                show_error(parent, "Invalid entry", &error.to_string());
                return;
            }
        }
    }
    let mut last_id: Option<Uuid> = None;
    {
        let mut vault = context.vault.borrow_mut();
        let data = vault.data_mut();
        for account in accounts {
            match data.add_account(account) {
                Ok(id) => last_id = Some(id),
                Err(error) => {
                    show_error(parent, "Invalid entry", &error.to_string());
                    return;
                }
            }
        }
    }
    if let Err(error) = context.vault.borrow().save() {
        show_error(parent, "Unable to save the vault", &error.to_string());
        return;
    }
    if let Some(id) = last_id {
        context.selected.set(Some(id));
    }
    render_accounts(context);
}

fn show_error(parent: &ApplicationWindow, title: &str, message: &str) {
    const COPY_RESPONSE: i32 = 100;
    let dialog = MessageDialog::builder()
        .buttons(ButtonsType::Ok)
        .text(title)
        .secondary_text(message)
        .build();
    let copy_button: Button = dialog
        .add_button("Copy", ResponseType::__Unknown(COPY_RESPONSE))
        .downcast()
        .expect("Copy button must be a Button widget");
    dialog.set_default_response(ResponseType::Ok);
    dialog.set_transient_for(Some(parent));
    let parent = parent.clone();
    let message = message.to_string();
    copy_button.connect_clicked(move |_| {
        copy_to_clipboard(&parent, &message);
    });
    dialog.connect_response(move |dialog, _| {
        dialog.close();
    });
    dialog.present();
}

fn show_about_dialog(parent: &ApplicationWindow) {
    // Plain `MessageDialog::builder()` would inherit the parent window's
    // title in its header bar; using a standalone `Dialog` keeps the chrome
    // tight and makes room for a readable, multi-line description.
    let dialog = Dialog::with_buttons(
        Some("About Notp"),
        Some(parent),
        DialogFlags::MODAL,
        &[("Close", ResponseType::Close)],
    );
    dialog.set_default_size(420, 220);
    if let Some(button) = dialog.widget_for_response(ResponseType::Close) {
        dialog.set_default_widget(Some(&button));
    }

    // Padding is set on each Label individually because `Dialog::content_area()`
    // drops its child margins in some Adwaita configurations — applying the
    // margin on the box alone would leave the labels flush against the edge.
    let content = GtkBox::new(Orientation::Vertical, 6);

    let title = Label::new(Some("Notp"));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    title.set_margin_start(24);
    title.set_margin_end(24);
    title.set_margin_top(18);
    content.append(&title);

    let tagline = Label::new(Some(
        "Native TOTP authenticator with an encrypted local vault.",
    ));
    tagline.set_xalign(0.0);
    tagline.set_wrap(true);
    tagline.add_css_class("dim-label");
    tagline.set_margin_start(24);
    tagline.set_margin_end(24);
    content.append(&tagline);

    let info = Label::new(None);
    info.set_xalign(0.0);
    info.set_wrap(true);
    info.set_selectable(true);
    info.set_margin_start(24);
    info.set_margin_end(24);
    info.set_margin_top(10);
    info.set_margin_bottom(18);
    info.set_markup(&format!(
        "<tt>Version  {version}\nGit tag  {tag}\nStorage  v{storage}</tt>",
        version = env!("CARGO_PKG_VERSION"),
        tag = env!("NOTP_GIT_TAG"),
        storage = CURRENT_VAULT_VERSION,
    ));
    content.append(&info);

    dialog.content_area().append(&content);
    dialog.connect_response(move |dialog, _| {
        dialog.close();
    });
    dialog.present();
}

#[allow(dead_code)]
fn show_application_error(application: &Application, title: &str, message: &str) {
    let application = application.clone();
    let window = ApplicationWindow::builder()
        .application(&application)
        .title(title)
        .default_width(420)
        .default_height(140)
        .build();
    let label = Label::new(Some(message));
    label.set_wrap(true);
    label.set_margin_start(20);
    label.set_margin_end(20);
    label.set_margin_top(20);
    label.set_margin_bottom(20);
    window.set_child(Some(&label));
    window.connect_close_request(move |_| {
        application.quit();
        glib::Propagation::Proceed
    });
    window.present();
}

fn copy_to_clipboard(window: &ApplicationWindow, text: &str) {
    set_clipboard_content(window, text);
}

#[derive(Copy, Clone)]
enum ClipboardTarget {
    Code,
    Secret,
}

fn copy_current_code(context: &RenderContext, window: &ApplicationWindow, target: ClipboardTarget) {
    let Some(id) = context.selected.get() else {
        return;
    };
    let snapshot = {
        let vault = context.vault.borrow();
        vault.data().account(id).map(|account| {
            (
                account.issuer.clone(),
                account.name.clone(),
                generate_code(
                    account.secret(),
                    current_timestamp(),
                    account.digits,
                    account.period,
                    account.algorithm,
                )
                .unwrap_or_else(|_| "------".to_string()),
                account.secret().to_string(),
                account.period,
            )
        })
    };
    let Some((issuer, name, code, secret, period)) = snapshot else {
        return;
    };
    match target {
        ClipboardTarget::Code => {
            copy_to_clipboard(window, &code);
            register_clipboard_auto_clear(window, code.clone());
            let remaining = remaining_seconds(current_timestamp(), period);
            show_toast(context, &format!("Code copied — expires in {} s", remaining));
        }
        ClipboardTarget::Secret => {
            copy_to_clipboard(window, &secret);
            register_clipboard_auto_clear(window, secret);
        }
    }
    if matches!(target, ClipboardTarget::Code) {
        // Record the user-initiated use immediately and refresh the detail
        // panel so the metadata label updates without waiting for the next
        // tick of the periodic refresh loop.
        {
            let mut vault = context.vault.borrow_mut();
            if let Some(account) = vault
                .data_mut()
                .accounts
                .iter_mut()
                .find(|account| account.id == id)
            {
                account.record_use();
            }
        }
        if let Err(error) = context.vault.borrow().save() {
            eprintln!("notp: failed to persist usage metadata: {error}");
        }
        show_selected(context);
        refresh_codes(context);
    }
    let _ = (issuer, name);
}

fn show_toast(context: &RenderContext, message: &str) {
    let Some(label) = context.toast.borrow().clone() else {
        return;
    };
    let Some(overlay_box) = context.toast_overlay.borrow().clone() else {
        return;
    };
    label.set_text(message);
    overlay_box.set_visible(true);
    glib::timeout_add_seconds_local_once(3, move || {
        overlay_box.set_visible(false);
    });
}

fn register_clipboard_auto_clear(window: &ApplicationWindow, value: String) {
    let settings = AppSettings::load().unwrap_or_default();
    let delay = settings.clipboard_clear_seconds;
    if delay == 0 {
        PENDING_CLIPBOARD_CLEAR.with(|state| {
            *state.borrow_mut() = None;
        });
        return;
    }
    let expires_at = Instant::now() + std::time::Duration::from_secs(delay);
    let window = window.clone();
    PENDING_CLIPBOARD_CLEAR.with(|state| {
        *state.borrow_mut() = Some(PendingClipboardClear {
            expires_at,
            value,
            window,
        });
    });
}

thread_local! {
    static PENDING_CLIPBOARD_CLEAR: RefCell<Option<PendingClipboardClear>> = const { RefCell::new(None) };
}

struct PendingClipboardClear {
    expires_at: Instant,
    value: String,
    window: ApplicationWindow,
}

fn poll_clipboard_auto_clear() -> bool {
    PENDING_CLIPBOARD_CLEAR.with(|state| {
        let mut slot = state.borrow_mut();
        let Some(pending) = slot.as_ref() else {
            return false;
        };
        if Instant::now() < pending.expires_at {
            return false;
        }
        let window = pending.window.clone();
        let expected = pending.value.clone();
        if read_clipboard_text(&window).as_deref() == Some(expected.as_str()) {
            clear_clipboard(&window);
        }
        *slot = None;
        true
    })
}

fn set_clipboard_content(window: &ApplicationWindow, text: &str) {
    let owned = text.to_owned();
    if copy_via_gdk(window, &owned).is_ok() {
        return;
    }
    let candidates = clipboard_command_candidates();
    for (command, args) in candidates {
        if let Ok(mut child) = std::process::Command::new(command)
            .args(args.iter().copied())
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                if stdin.write_all(owned.as_bytes()).is_ok() {
                    let _ = child.wait();
                    return;
                }
            }
            let _ = child.kill();
        }
    }
}

fn copy_via_gdk(window: &ApplicationWindow, text: &str) -> std::result::Result<(), ()> {
    use gdk4::prelude::DisplayExt;
    let display = gtk::prelude::RootExt::display(window);
    let clipboard = display.clipboard();
    clipboard.set_text(text);
    Ok(())
}

fn read_clipboard_text(window: &ApplicationWindow) -> Option<String> {
    use gdk4::prelude::DisplayExt;
    let display = gtk::prelude::RootExt::display(window);
    let clipboard = display.clipboard();
    let provider = clipboard.content()?;
    let value: gtk::glib::Value = provider.value(gtk::glib::Type::STRING).ok()?;
    value.get::<String>().ok()
}

fn clear_clipboard(window: &ApplicationWindow) {
    use gdk4::prelude::DisplayExt;
    let display = gtk::prelude::RootExt::display(window);
    let clipboard = display.clipboard();
    clipboard.set_text("");
}

fn clipboard_command_candidates() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut candidates = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    {
        candidates.push(("wl-copy", Vec::new()));
    }
    candidates.push(("xclip", vec!["-selection", "clipboard"]));
    candidates.push(("xsel", vec!["--clipboard", "--input"]));
    candidates
}

fn select_first_or_last(list_box: &ListBox, first: bool) {
    let total = list_box.observe_children().n_items();
    if total == 0 {
        return;
    }
    let index = if first { 0 } else { (total - 1) as i32 };
    if let Some(row) = list_box.row_at_index(index) {
        list_box.select_row(Some(&row));
    }
}

fn request_delete(context: &RenderContext, window: &ApplicationWindow, id: Uuid) {
    let snapshot = {
        let vault = context.vault.borrow();
        vault
            .data()
            .account(id)
            .map(|account| (account.issuer.clone(), account.name.clone()))
    };
    let Some((issuer, name)) = snapshot else {
        return;
    };
    let message = format!("Delete the entry « {} » ({}) ?", name, issuer);
    let context_for_confirmation = context.clone();
    let window_for_confirmation = window.clone();
    confirm(&window_for_confirmation.clone(), "Delete entry", &message, move || {
        let result = context_for_confirmation
            .vault
            .borrow_mut()
            .remove_account(id);
        if let Err(error) = result {
            show_error(&window_for_confirmation, "Unable to save the vault", &error.to_string());
        } else {
            context_for_confirmation.selected.set(None);
            render_accounts(&context_for_confirmation);
        }
    });
}

fn edit_selected(context: &RenderContext, window: &ApplicationWindow, id: Uuid) {
    let snapshot = {
        let vault = context.vault.borrow();
        vault.data().account(id).map(|account| {
            (
                account.issuer.clone(),
                account.name.clone(),
                account.secret().to_string(),
                account.digits,
                account.period,
                account.algorithm,
            )
        })
    };
    let Some((issuer, name, secret, digits, period, algorithm)) = snapshot else {
        return;
    };
    let params = OtpParams {
        issuer,
        label: name,
        secret,
        digits,
        period,
        algorithm,
    };
    let context_for_save = context.clone();
    let window_for_save = window.clone();
    add_account_dialog(window, Some(params), move |account| {
        let Some(mut account) = account else {
            return;
        };
        let result = {
            let mut vault = context_for_save.vault.borrow_mut();
            let data = vault.data_mut();
            if let Some(position) = data.accounts.iter().position(|existing| existing.id == id) {
                // Preserve the stable identity and usage history when
                // overwriting in place so editing an entry does not change
                // its id (used for the selection) or reset its counters.
                account.inherit_identity_from(&data.accounts[position]);
                data.accounts[position] = account;
            } else {
                if let Err(error) = data.add_account(account) {
                    drop(vault);
                    show_error(&window_for_save, "Invalid entry", &error.to_string());
                    return;
                }
            }
            vault.save()
        };
        match result {
            Ok(()) => {
                context_for_save.selected.set(Some(id));
                render_accounts(&context_for_save);
            }
            Err(error) => show_error(
                &window_for_save,
                "Unable to save the vault",
                &error.to_string(),
            ),
        }
    });
}

fn preferences_dialog<F>(parent: &ApplicationWindow, current: AppSettings, on_apply: F)
where
    F: FnOnce(AppSettings) + 'static,
{
    let parent_for_error = parent.clone();
    let dialog = Dialog::with_buttons(
        Some("Preferences"),
        Some(parent),
        DialogFlags::MODAL,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Apply", ResponseType::Accept),
        ],
    );
    dialog.set_default_size(500, 280);
    if let Some(button) = dialog.widget_for_response(ResponseType::Accept) {
        button.add_css_class("suggested-action");
        dialog.set_default_widget(Some(&button));
    }

    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_margin_start(20);
    container.set_margin_end(20);
    container.set_margin_top(18);
    container.set_margin_bottom(18);

    let grid = Grid::new();
    grid.set_row_spacing(10);
    grid.set_column_spacing(12);
    grid.set_hexpand(true);

    let auto_lock_adjustment = Adjustment::new(
        current.auto_lock_seconds as f64,
        MIN_AUTO_LOCK_SECONDS as f64,
        MAX_AUTO_LOCK_SECONDS as f64,
        1.0,
        10.0,
        0.0,
    );
    let auto_lock = SpinButton::new(Some(&auto_lock_adjustment), 1.0, 0);
    auto_lock.set_value(current.auto_lock_seconds as f64);
    auto_lock.set_hexpand(true);
    auto_lock.set_halign(gtk::Align::End);
    auto_lock.set_width_chars(8);

    let clipboard_adjustment = Adjustment::new(
        current.clipboard_clear_seconds as f64,
        MIN_CLIPBOARD_CLEAR_SECONDS as f64,
        MAX_CLIPBOARD_CLEAR_SECONDS as f64,
        1.0,
        5.0,
        0.0,
    );
    let clipboard_clear = SpinButton::new(Some(&clipboard_adjustment), 1.0, 0);
    clipboard_clear.set_value(current.clipboard_clear_seconds as f64);
    clipboard_clear.set_hexpand(true);
    clipboard_clear.set_halign(gtk::Align::End);
    clipboard_clear.set_width_chars(8);
    let clipboard_hint = Label::new(Some("(0 disables auto-clear)"));
    clipboard_hint.set_xalign(1.0);
    clipboard_hint.set_margin_top(2);
    clipboard_hint.add_css_class("dim-label");

    let theme_combo = ComboBoxText::new();
    theme_combo.append(Some("system"), "Follow system");
    theme_combo.append(Some("light"), "Light");
    theme_combo.append(Some("dark"), "Dark");
    theme_combo.set_active_id(Some(match current.theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
        Theme::System => "system",
    }));
    theme_combo.set_hexpand(true);
    theme_combo.set_halign(gtk::Align::End);

    let mut row = 0;
    add_field(
        &grid,
        &Label::new(Some("Auto-lock after (seconds)")),
        &auto_lock,
        row,
    );
    row += 1;
    add_field(
        &grid,
        &Label::new(Some("Auto-clear clipboard (seconds)")),
        &clipboard_clear,
        row,
    );
    grid.attach(&clipboard_hint, 1, row, 1, 1);
    row += 1;
    add_field(&grid, &Label::new(Some("Theme")), &theme_combo, row);

    container.append(&grid);
    dialog.content_area().append(&container);

    dialog.run_async(move |dialog, response| {
        if response == ResponseType::Accept {
            let theme = match theme_combo.active_id().as_deref() {
                Some("light") => Theme::Light,
                Some("dark") => Theme::Dark,
                _ => Theme::System,
            };
            let mut updated = current;
            updated.auto_lock_seconds = auto_lock.value() as u64;
            updated.clipboard_clear_seconds = clipboard_clear.value() as u64;
            updated.theme = theme;
            on_apply(updated.normalized());
        }
        dialog.close();
    });
    let _ = parent_for_error;
}

fn change_password_dialog<F>(parent: &ApplicationWindow, on_apply: F)
where
    F: FnOnce(&str, &str) + 'static,
{
    let parent_for_error = parent.clone();
    let dialog = Dialog::with_buttons(
        Some("Change master password"),
        Some(parent),
        DialogFlags::MODAL,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Change", ResponseType::Accept),
        ],
    );
    dialog.set_default_size(500, 260);
    if let Some(button) = dialog.widget_for_response(ResponseType::Accept) {
        button.add_css_class("suggested-action");
        dialog.set_default_widget(Some(&button));
    }

    let container = GtkBox::new(Orientation::Vertical, 0);
    container.set_margin_start(20);
    container.set_margin_end(20);
    container.set_margin_top(18);
    container.set_margin_bottom(18);

    let grid = Grid::new();
    grid.set_row_spacing(10);
    grid.set_column_spacing(12);
    grid.set_hexpand(true);

    let current_entry = Entry::new();
    current_entry.set_visibility(false);
    current_entry.set_input_purpose(gtk::InputPurpose::Password);
    current_entry.set_placeholder_text(Some("Current master password"));
    current_entry.set_hexpand(true);
    let new_entry = Entry::new();
    new_entry.set_visibility(false);
    new_entry.set_input_purpose(gtk::InputPurpose::Password);
    new_entry.set_placeholder_text(Some("New password (8 chars minimum)"));
    new_entry.set_hexpand(true);
    let confirm_entry = Entry::new();
    confirm_entry.set_visibility(false);
    confirm_entry.set_input_purpose(gtk::InputPurpose::Password);
    confirm_entry.set_placeholder_text(Some("Confirm new password"));
    confirm_entry.set_hexpand(true);

    let mut row = 0;
    add_field(&grid, &Label::new(Some("Current")), &current_entry, row);
    row += 1;
    add_field(&grid, &Label::new(Some("New")), &new_entry, row);
    row += 1;
    add_field(&grid, &Label::new(Some("Confirm")), &confirm_entry, row);
    container.append(&grid);
    dialog.content_area().append(&container);

    dialog.run_async(move |dialog, response| {
        if response != ResponseType::Accept {
            dialog.close();
            return;
        }
        let current = current_entry.text().to_string();
        let new_password = new_entry.text().to_string();
        let confirmation = confirm_entry.text().to_string();
        if new_password != confirmation {
            show_error(
                &parent_for_error,
                "Invalid new password",
                "Password confirmation does not match",
            );
            dialog.close();
            return;
        }
        on_apply(&current, &new_password);
        dialog.close();
    });
}

fn apply_theme(theme: &Theme) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_property("gtk-application-prefer-dark-theme", matches!(theme, Theme::Dark));
    }
    let provider = CssProvider::new();
    provider.load_from_data(NOTP_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

const NOTP_CSS: &str = "
.notp-toast {
    padding: 10px 16px;
    border-radius: 8px;
    background-color: alpha(#000000, 0.85);
    color: #ffffff;
}
";
