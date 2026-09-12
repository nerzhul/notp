use crate::otp::{current_timestamp, generate_code, remaining_seconds, Algorithm};
use crate::qr_import::OtpParams;
use crate::settings::AppSettings;
use crate::storage::{Account, Vault, VaultStore};
use gtk::gdk::{ContentProvider, DragAction};
use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, ButtonsType, ComboBoxText,
    Dialog, DialogFlags, DragSource, DropTarget, Entry, Grid, HeaderBar, Label, ListBox,
    ListBoxRow, MenuButton, MessageDialog, Orientation, Paned, Popover, ResponseType,
    ScrolledWindow, SelectionMode, SpinButton, Spinner, Stack, WidgetPaintable,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

const AUTO_LOCK_SECONDS: u64 = 60;
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
    countdown: Label,
    copy: Button,
    remove: Button,
}

#[derive(Clone)]
struct RenderContext {
    vault: Rc<RefCell<Vault>>,
    list_box: ListBox,
    row_labels: Rc<RefCell<HashMap<Uuid, Label>>>,
    detail: DetailWidgets,
    selected: Rc<Cell<Option<Uuid>>>,
}

fn show_main_window(application: &Application, vault: Vault) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Notp")
        .default_width(900)
        .default_height(600)
        .build();

    let vault = Rc::new(RefCell::new(vault));
    let selected = Rc::new(Cell::new(None::<Uuid>));
    let row_labels = Rc::new(RefCell::new(HashMap::new()));

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
    lock_button.set_tooltip_text(Some("Lock the vault"));
    lock_button.set_valign(gtk::Align::Center);

    header_box.append(&add_button);
    header_box.append(&import_menu_button);
    header_box.append(&lock_button);
    header.pack_start(&header_box);
    window.set_titlebar(Some(&header));

    let list_scroller = ScrolledWindow::new();
    list_scroller.set_min_content_width(280);
    list_scroller.set_hexpand(true);
    list_scroller.set_vexpand(true);
    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);
    list_box.set_activate_on_single_click(false);
    list_scroller.set_child(Some(&list_box));

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
    let detail_countdown = Label::new(None);
    detail_countdown.set_xalign(0.0);
    detail_content.append(&detail_countdown);

    let action_box = GtkBox::new(Orientation::Horizontal, 8);
    let copy_button = Button::with_label("Copy code");
    copy_button.set_sensitive(false);
    let remove_button = Button::with_label("Delete");
    remove_button.set_sensitive(false);
    action_box.append(&copy_button);
    action_box.append(&remove_button);
    detail_content.append(&action_box);
    detail_stack.add_named(&detail_content, Some("content"));
    detail_stack.set_visible_child_name("empty");

    let paned = Paned::new(Orientation::Horizontal);
    paned.set_vexpand(true);
    paned.set_resize_start_child(true);
    paned.set_shrink_start_child(false);
    paned.set_resize_end_child(true);
    paned.set_shrink_end_child(false);
    paned.set_start_child(Some(&list_scroller));
    paned.set_end_child(Some(&detail_stack));
    paned.set_position(300);

    let main_box = GtkBox::new(Orientation::Vertical, 0);
    main_box.append(&paned);
    window.set_child(Some(&main_box));

    let detail = DetailWidgets {
        stack: detail_stack,
        issuer: detail_issuer,
        account: detail_account,
        code: detail_code,
        countdown: detail_countdown,
        copy: copy_button,
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
            copy: detail.copy.clone(),
            remove: detail.remove.clone(),
        },
        selected: selected.clone(),
    };

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
        let Some(id) = context_for_copy.selected.get() else {
            return;
        };
        let code = {
            let vault = context_for_copy.vault.borrow();
            vault.data().account(id).and_then(|account| {
                generate_code(
                    account.secret(),
                    current_timestamp(),
                    account.digits,
                    account.period,
                    account.algorithm,
                )
                .ok()
            })
        };
        if let Some(code) = code {
            copy_to_clipboard(&window_for_copy, &code);
        }
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
    glib::timeout_add_seconds_local(1, move || {
        refresh_codes(&context);
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
    glib::timeout_add_seconds_local(1, move || {
        if has_been_active_for_timer.get() && !window_for_timer.is_active() {
            let now = Instant::now();
            let last = last_inactive_for_timer
                .get()
                .unwrap_or_else(|| {
                    last_inactive_for_timer.set(Some(now));
                    now
                });
            if now.duration_since(last).as_secs() >= AUTO_LOCK_SECONDS {
                lock_action_for_timer();
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    });

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

    let accounts = {
        let vault = context.vault.borrow();
        vault
            .data()
            .accounts
            .iter()
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

    let selected = selected.or(first_id);
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
        context.detail.remove.set_sensitive(false);
        return;
    };
    let vault = context.vault.borrow();
    let Some(account) = vault.data().account(id) else {
        drop(vault);
        context.selected.set(None);
        context.detail.stack.set_visible_child_name("empty");
        context.detail.copy.set_sensitive(false);
        context.detail.remove.set_sensitive(false);
        return;
    };
    context.detail.issuer.set_text(&account.issuer);
    context.detail.account.set_text(&account.name);
    context.detail.stack.set_visible_child_name("content");
    context.detail.copy.set_sensitive(true);
    context.detail.remove.set_sensitive(true);
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
            context.detail.countdown.set_text(&format!(
                "Expires in {} s",
                remaining_seconds(timestamp, account.period)
            ));
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
