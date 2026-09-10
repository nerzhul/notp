use crate::otp::{current_timestamp, generate_code, remaining_seconds, Algorithm};
use crate::storage::{Account, Vault, VaultStore};
use gtk::prelude::*;
use gtk::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, ButtonsType, ComboBoxText,
    Dialog, DialogFlags, Entry, Grid, HeaderBar, Label, ListBox, ListBoxRow, MessageDialog,
    Orientation, Paned, ResponseType, ScrolledWindow, SelectionMode, SpinButton, Stack,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use uuid::Uuid;

const APPLICATION_ID: &str = "com.nerzhul.notp";

pub fn run() {
    gtk::init().expect("Unable to initialize GTK");
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .build();

    application.connect_activate(move |app| {
        let store = match VaultStore::new() {
            Ok(store) => store,
            Err(error) => {
                show_application_error(app, "Storage unavailable", &error.to_string());
                return;
            }
        };
        if store.exists() {
            show_unlock_window(app, store);
        } else {
            show_setup_window(app, store);
        }
    });

    application.run();
}

fn show_setup_window(application: &Application, store: VaultStore) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Notp — Setup")
        .default_width(460)
        .default_height(300)
        .build();

    let dialog = Dialog::with_buttons(
        Some("Create vault"),
        Some(&window),
        DialogFlags::MODAL,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Create", ResponseType::Accept),
        ],
    );
    dialog.set_default_size(440, 260);

    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    let explanation = Label::new(Some(
        "Choose a master password. It is not stored and cannot be recovered.",
    ));
    explanation.set_wrap(true);
    explanation.set_xalign(0.0);
    content.append(&explanation);

    let password = Entry::new();
    password.set_placeholder_text(Some("Master password (8 characters minimum)"));
    password.set_visibility(false);
    password.set_input_purpose(gtk::InputPurpose::Password);
    content.append(&password);

    let confirmation = Entry::new();
    confirmation.set_placeholder_text(Some("Confirm master password"));
    confirmation.set_visibility(false);
    confirmation.set_input_purpose(gtk::InputPurpose::Password);
    content.append(&confirmation);

    let error_label = Label::new(None);
    error_label.set_xalign(0.0);
    error_label.add_css_class("error");
    content.append(&error_label);
    dialog.content_area().append(&content);

    let application_for_response = application.clone();
    let application_for_quit = application.clone();
    let application_for_close = application.clone();
    let store_for_response = store.clone();
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            let password_text = password.text();
            let confirmation_text = confirmation.text();
            if password_text != confirmation_text {
                error_label.set_text("Passwords do not match");
                return;
            }
            match store_for_response.create(&password_text) {
                Ok(vault) => {
                    dialog.destroy();
                    window.destroy();
                    show_main_window(&application_for_response, vault);
                }
                Err(error) => {
                    error_label.set_text(&error.to_string());
                }
            }
        } else {
            application_for_quit.quit();
        }
    });

    window.connect_close_request(move |_| {
        application_for_close.quit();
        glib::Propagation::Proceed
    });
    dialog.present();
    window.present();
}

fn show_unlock_window(application: &Application, store: VaultStore) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Notp — Unlock")
        .default_width(460)
        .default_height(240)
        .build();

    let dialog = Dialog::with_buttons(
        Some("Unlock Notp"),
        Some(&window),
        DialogFlags::MODAL,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Unlock", ResponseType::Accept),
        ],
    );
    dialog.set_default_size(440, 190);

    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    let explanation = Label::new(Some("Enter your master password to open the vault."));
    explanation.set_wrap(true);
    explanation.set_xalign(0.0);
    content.append(&explanation);

    let password = Entry::new();
    password.set_placeholder_text(Some("Master password"));
    password.set_visibility(false);
    password.set_input_purpose(gtk::InputPurpose::Password);
    content.append(&password);

    let error_label = Label::new(None);
    error_label.set_xalign(0.0);
    error_label.add_css_class("error");
    content.append(&error_label);
    dialog.content_area().append(&content);

    let application_for_response = application.clone();
    let application_for_quit = application.clone();
    let application_for_close = application.clone();
    let store_for_response = store.clone();
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            let password_text = password.text();
            match store_for_response.unlock(&password_text) {
                Ok(vault) => {
                    dialog.destroy();
                    window.destroy();
                    show_main_window(&application_for_response, vault);
                }
                Err(_) => {
                    password.set_text("");
                    password.grab_focus();
                    error_label.set_text("Incorrect password or corrupted vault");
                }
            }
        } else {
            application_for_quit.quit();
        }
    });

    window.connect_close_request(move |_| {
        application_for_close.quit();
        glib::Propagation::Proceed
    });
    dialog.present();
    window.present();
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
    let add_button = Button::with_label("Add entry");
    header.pack_start(&add_button);
    window.set_titlebar(Some(&header));

    let list_scroller = ScrolledWindow::new();
    list_scroller.set_min_content_width(280);
    list_scroller.set_hexpand(true);
    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);
    list_box.set_activate_on_single_click(false);
    list_scroller.set_child(Some(&list_box));

    let detail_stack = Stack::new();
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
        add_account_dialog(&window_for_add, move |account| {
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
    window.connect_close_request(move |_| {
        application.quit();
        glib::Propagation::Proceed
    });
    window.present();
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

fn add_account_dialog<F>(parent: &ApplicationWindow, on_result: F)
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

fn show_error(parent: &ApplicationWindow, title: &str, message: &str) {
    let dialog = MessageDialog::builder()
        .buttons(ButtonsType::Ok)
        .text(message)
        .secondary_text(title)
        .build();
    dialog.set_transient_for(Some(parent));
    dialog.run_async(|dialog, _| dialog.close());
}

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
    let display = gtk::prelude::RootExt::display(window);
    let clipboard = gdk4::Display::clipboard(&display);
    clipboard.set_text(text);
}
