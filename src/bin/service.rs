// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, you can obtain one at https://mozilla.org/MPL/2.0/.

//! The main process for fxa-email-service.
//! Starts a Rocket server
//! that exposes one endpoint: `POST /send`
//!
//! Configuration is via [`settings::Settings`][settings].
//! By default the server listens on `127.0.0.1:8001`.
//!
//! [settings]: ../fxa_email_service/settings/struct.Settings.html

#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate fxa_email_service;
extern crate rocket;
#[macro_use(
    slog_b,
    slog_info,
    slog_kv,
    slog_log,
    slog_record,
    slog_record_static
)]
extern crate slog;
#[macro_use]
extern crate sentry;

use sentry::integrations::panic::register_panic_handler;

use fxa_email_service::{
    app_errors, auth_db::DbClient, bounces::Bounces, healthcheck, logging::MozlogLogger,
    message_data::MessageData, providers::Providers, send, settings::Settings,
};

fn main() {
    let settings = Settings::new().expect("Settings::new error");
    let db = DbClient::new(&settings);
    let bounces = Bounces::new(&settings, db);
    let logger = MozlogLogger::new(&settings).expect("MozlogLogger::init error");
    let message_data = MessageData::new(&settings);
    let providers = Providers::new(&settings);

    if let Some(ref sentry) = settings.sentry {
        sentry::init((
            sentry.dsn.0.clone(),
            sentry::ClientOptions {
                release: sentry_crate_release!(),
                ..Default::default()
            },
        ));
        register_panic_handler();
    }

    let config = settings
        .build_rocket_config()
        .expect("Error creating rocket config");
    rocket::custom(config)
        .manage(settings)
        .manage(bounces)
        .manage(logger)
        .manage(message_data)
        .manage(providers)
        .mount(
            "/",
            routes![
                send::handler,
                healthcheck::heartbeat,
                healthcheck::lbheartbeat,
                healthcheck::version
            ],
        )
        .catch(catchers![
            app_errors::bad_request,
            app_errors::not_found,
            app_errors::method_not_allowed,
            app_errors::unprocessable_entity,
            app_errors::too_many_requests,
            app_errors::internal_server_error
        ])
        .attach(rocket::fairing::AdHoc::on_request(|request, _| {
            let log =
                MozlogLogger::with_request(request).expect("MozlogLogger::with_request error");
            slog_info!(log, "{}", "Request started");
        }))
        .attach(rocket::fairing::AdHoc::on_response(|request, response| {
            let log =
                MozlogLogger::with_request(request).expect("MozlogLogger::with_request error");
            if response.status().code == 200 {
                slog_info!(log, "{}", "Request finished succesfully";
                    "status_code" => response.status().code, "status_msg" => response.status().reason);
            }
        }))
        .launch();
}
