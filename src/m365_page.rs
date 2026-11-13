//! The Microsoft 365 page of Settings, and keeping the account, its sign-in and the calendar in step with the settings.

use super::auth::SignInError;
use super::graph::{GraphError, Me};
use super::{Feature, Session, Store};
use crate::customize::{card, heading, page_header};
use crate::i18n::tr;
use crate::search_page::switch_line;
use crate::ui::Menu;
use crate::widgets::*;
use egui::*;
use std::sync::{Arc, Mutex};

/// Why signing in didn't work, as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// An administrator has to approve Startline for the organization.
    AdminConsent,
    Message(String),
}

#[derive(Default)]
pub struct M365Ui {
    signing_in: bool,
    signed: Arc<Mutex<Option<Result<String, SignInError>>>>,
    pub problem: Option<Problem>,
    /// Who's signed in, or why Graph wouldn't say: asked once per session and after the features change.
    who: Arc<Mutex<Option<Result<Me, GraphError>>>>,
    shown: Option<Result<Me, GraphError>>,
    asked_for: Option<Vec<&'static str>>,
    copied: bool,
}

impl M365Ui {
    /// For a picture: shows this account as signed in.
    pub fn show_demo(&mut self, me: Me) {
        self.shown = Some(Ok(me));
    }
}

impl Menu {
    /// Opens the session the settings describe, closes one they no longer allow, and takes in sign-in answers.
    pub(crate) fn keep_m365(&mut self, ctx: &Context) {
        if let Some(answer) = self.m365_ui.signed.lock().unwrap().take() {
            self.m365_ui.signing_in = false;
            match answer {
                Ok(account) => {
                    self.cfg.m365_account_id = account;
                    self.m365_ui.problem = None;
                }
                Err(SignInError::Cancelled) => {}
                Err(SignInError::AdminConsent) => self.m365_ui.problem = Some(Problem::AdminConsent),
                Err(SignInError::NoBroker) => self.m365_ui.problem = Some(Problem::Message(tr("Windows couldn't sign in, and neither could the browser.").into())),
                Err(SignInError::Failed(message)) => self.m365_ui.problem = Some(Problem::Message(message)),
            }
        }
        if self.is_shot() {
            return;
        }
        let wanted = super::signed_in(&self.cfg);
        let fits = self.m365.as_ref().is_some_and(|s| s.fits(&self.cfg));
        if wanted && !fits {
            self.m365 = Session::open(&self.cfg, Store::app()).map(Arc::new);
            self.calendar = super::panel::Calendar::default();
            self.m365_ui.asked_for = None;
        } else if !wanted && self.m365.is_some() {
            self.m365 = None;
            self.calendar = super::panel::Calendar::default();
        }
        if let Some(session) = self.m365.clone() {
            session.set_scopes(&self.cfg);
            // Ask who's signed in again whenever what's asked for changes: a new permission may need the person.
            let scopes = session.scopes();
            if self.m365_ui.asked_for.as_ref() != Some(&scopes) {
                self.m365_ui.asked_for = Some(scopes);
                *session.me.lock().unwrap() = None;
                let (slot, ctx) = (self.m365_ui.who.clone(), ctx.clone());
                std::thread::spawn(move || {
                    crate::platform::com_init();
                    *slot.lock().unwrap() = Some(session.who());
                    ctx.request_repaint();
                });
            }
            if let Some(answer) = self.m365_ui.who.lock().unwrap().take() {
                if answer == Err(GraphError::AdminConsent) {
                    self.m365_ui.problem = Some(Problem::AdminConsent);
                }
                self.m365_ui.shown = Some(answer);
            }
        }
        if super::feature_on(&self.cfg, Feature::Calendar) {
            let session = self.m365.clone();
            self.calendar.tick(session.as_ref(), ctx);
        }
    }

    fn m365_sign_in(&mut self, ctx: &Context) {
        if self.m365_ui.signing_in {
            return;
        }
        self.m365_ui.signing_in = true;
        self.m365_ui.problem = None;
        self.m365_ui.copied = false;
        let hwnd = crate::platform::window_by_title(tr(crate::customize::WINDOW_TITLE)).unwrap_or_else(|| self.shared.window_id());
        let (cfg, slot, ctx) = (self.cfg.clone(), self.m365_ui.signed.clone(), ctx.clone());
        std::thread::spawn(move || {
            crate::platform::com_init();
            *slot.lock().unwrap() = Some(super::sign_in(hwnd, &cfg, &Store::app()));
            ctx.request_repaint();
        });
    }

    /// Forgets the account and everything of its sign-in.
    pub(crate) fn m365_sign_out(&mut self) {
        match self.m365.take() {
            Some(session) => session.sign_out(&mut self.cfg),
            None => super::forget_everything(&mut self.cfg, &Store::app()),
        }
        self.calendar = super::panel::Calendar::default();
        self.m365_ui = M365Ui::default();
        // People and mail already found go too.
        use crate::providers::Provider;
        self.providers.people.clear();
        self.providers.mail.clear();
        self.assemble_rows();
    }
}

/// Who set the app registration or tenant in use.
pub fn source_label(from: super::Source) -> &'static str {
    match from {
        super::Source::Organization => tr("Set by your organization"),
        super::Source::Installer => tr("Set by the installer"),
        super::Source::BuiltIn => tr("Built in"),
    }
}

pub fn draw(menu: &mut Menu, ui: &mut Ui, pal: Pal) {
    let policy = menu.policy.clone();
    page_header(ui, tr("Microsoft 365"), tr("An optional work or school account: your calendar beside the menu, and people and mail in search. Off until you turn it on."), pal);
    if !super::available(&menu.cfg) {
        card(ui, pal, |ui| {
            ui.label(RichText::new(tr("Not available in this build")).font(semibold(14.0)).color(pal.text));
            ui.label(RichText::new(tr("No Microsoft 365 app registration is set. It can be built in, given when installing (M365CLIENTID), or set by an organization with the M365ClientId policy.")).size(12.5).color(pal.sub));
        });
        return;
    }

    heading(ui, tr("Account"), pal);
    let ctx = ui.ctx().clone();
    card(ui, pal, |ui| {
        policy.lock(ui, &["M365Account"], |ui| switch_line(ui, tr("Use a Microsoft 365 account"), tr("Sign in with a work or school account. Windows keeps the sign-in; Startline keeps only which account it is."), &mut menu.cfg.m365, pal));
        if !menu.cfg.m365 {
            return;
        }
        ui.separator();
        let signed_in = super::signed_in(&menu.cfg);
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            let status = if menu.m365_ui.signing_in {
                tr("Signing in... Finish in the window that opened.").to_string()
            } else if signed_in {
                match &menu.m365_ui.shown {
                    Some(Ok(me)) if !me.mail.is_empty() => crate::trf!("Signed in as {name} ({mail})", name = me.name, mail = me.mail),
                    Some(Ok(me)) => crate::trf!("Signed in as {name}", name = me.name),
                    _ => tr("Signed in").to_string(),
                }
            } else {
                tr("Not signed in").to_string()
            };
            ui.label(RichText::new(status).size(13.5).color(pal.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if signed_in {
                    if ui.button(tr("Sign out")).clicked() {
                        menu.m365_sign_out();
                    }
                } else if !menu.m365_ui.signing_in && ui.button(tr("Sign in")).clicked() {
                    menu.m365_sign_in(&ctx);
                }
            });
        });
        // A permission switched on since the sign-in may need the person to agree to it.
        if signed_in && matches!(menu.m365_ui.shown, Some(Err(GraphError::SignInNeeded))) && menu.m365_ui.problem.is_none() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Microsoft 365 needs you to sign in again, to allow what's switched on.")).size(12.5).color(pal.sub));
                if ui.button(tr("Sign in again")).clicked() {
                    menu.m365_sign_in(&ctx);
                }
            });
        }
        match menu.m365_ui.problem.clone() {
            Some(Problem::AdminConsent) => {
                ui.label(RichText::new(tr("Your organization has to approve Startline before you can use it. Send your administrator the approval link; once they've approved it, sign in again.")).size(12.5).color(Color32::from_rgb(222, 120, 96)));
                ui.horizontal(|ui| {
                    if let Some(link) = super::admin_consent_link(&menu.cfg) {
                        if ui.button(tr("Copy the approval link")).clicked() {
                            let _ = crate::platform::copy_text(&link);
                            menu.m365_ui.copied = true;
                        }
                    }
                    if menu.m365_ui.copied {
                        ui.label(RichText::new(tr("Copied")).size(12.5).color(pal.sub));
                    }
                });
            }
            Some(Problem::Message(message)) => {
                ui.label(RichText::new(crate::trf!("The sign-in didn't work: {message}", message = message)).size(12.5).color(Color32::from_rgb(222, 120, 96)));
            }
            None => {}
        }
    });

    heading(ui, tr("What Startline may use"), pal);
    let on = menu.cfg.m365;
    card(ui, pal, |ui| {
        ui.add_enabled_ui(on, |ui| {
            policy.lock(ui, &["M365Calendar"], |ui| switch_line(ui, tr("Calendar beside the menu"), tr("Today's calendar in a panel on the right of the menu. Only in the Windows 11 and Startline looks: Windows 10, Windows 7 and Windows XP don't show it yet."), &mut menu.cfg.m365_calendar, pal));
            policy.lock(ui, &["M365People"], |ui| switch_line(ui, tr("People in search"), tr("Colleagues in search: Enter starts a Teams chat, and buttons call or mail them."), &mut menu.cfg.m365_people, pal));
            policy.lock(ui, &["M365Mail"], |ui| switch_line(ui, tr("Mail in search"), tr("Your mail in search, by subject. Enter opens it in Outlook on the web."), &mut menu.cfg.m365_mail, pal));
        });
        ui.label(RichText::new(tr("Microsoft is asked only for what's switched on here.")).size(12.0).color(pal.sub));
    });

    // Which app registration and tenant are in use, and who set them. Only an organization, the installer or the build
    // sets them; see the admins' part of the README.
    heading(ui, tr("App registration"), pal);
    card(ui, pal, |ui| {
        let line = |ui: &mut Ui, title: &str, value: String, from: Option<super::Source>| {
            ui.horizontal(|ui| {
                ui.set_min_height(30.0);
                ui.label(RichText::new(title).size(13.5).color(pal.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(from) = from {
                        ui.label(RichText::new(source_label(from)).size(12.0).color(pal.sub));
                    }
                    ui.label(RichText::new(value).size(12.5).color(pal.text));
                });
            });
        };
        if let Some((id, from)) = super::client_source(&menu.cfg) {
            line(ui, tr("App registration (client ID)"), id, Some(from));
        }
        match super::tenant_source(&menu.cfg) {
            Some((tenant, from)) => line(ui, tr("Tenant"), tenant, Some(from)),
            None => line(ui, tr("Tenant"), tr("Any work or school account").to_string(), None),
        }
    });
    ui.label(RichText::new(tr("Startline talks only to Microsoft's sign-in and to Microsoft Graph, and sends nothing anywhere else.")).size(12.0).color(pal.sub));
}
