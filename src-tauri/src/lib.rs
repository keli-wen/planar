use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use planar_core::db::Store;
use planar_core::env::{HarnessRoots, default_db_path, hydrate_macos_path};
use planar_core::server::{AppState, build_embedded_router, serve_until};
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, Url, WindowEvent};
use tokio::sync::watch;

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Launches the standalone Planar desktop and menu-bar application.
pub fn run() -> Result<()> {
    let hydrated_path = hydrate_macos_path();
    let db_path = default_db_path().context("failed to resolve planar SQLite path")?;
    let roots = HarnessRoots::from_env_or_home().context("failed to resolve harness log roots")?;

    Store::open(&db_path).context("failed to initialize planar SQLite store")?;

    let std_listener = TcpListener::bind("127.0.0.1:4310")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .context("failed to bind local planar HTTP listener")?;
    std_listener
        .set_nonblocking(true)
        .context("failed to configure non-blocking listener")?;
    let local_addr = std_listener
        .local_addr()
        .context("failed to inspect bound local address")?;
    let ui_url = Url::parse(&format!("http://{local_addr}/ui/"))
        .context("failed to construct loopback UI URL")?;

    let app_state = AppState::with_hydrated_path(db_path, roots, hydrated_path);
    let quitting = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = watch::channel(false);

    let quitting_setup = Arc::clone(&quitting);
    let stop_tx_setup = stop_tx.clone();
    let state_setup = app_state.clone();

    let app = tauri::Builder::default()
        .setup(move |app| {
            let server_state = state_setup.clone();
            let mut server_stop = stop_rx.clone();
            tauri::async_runtime::spawn(async move {
                match tokio::net::TcpListener::from_std(std_listener) {
                    Ok(listener) => {
                        let router = build_embedded_router(server_state);
                        let shutdown = async move {
                            while !*server_stop.borrow() {
                                if server_stop.changed().await.is_err() {
                                    break;
                                }
                            }
                        };
                        if let Err(err) = serve_until(listener, router, shutdown).await {
                            eprintln!("planar embedded server stopped: {err}");
                        }
                    }
                    Err(err) => {
                        eprintln!("failed to adopt HTTP listener into tokio: {err}");
                    }
                }
            });

            let bg_state = state_setup.clone();
            let mut bg_stop = stop_rx;
            tauri::async_runtime::spawn(async move {
                if let Err(err) = bg_state.run_scan().await {
                    eprintln!("planar startup scan warning: {err}");
                }
                let mut ticker = tokio::time::interval(Duration::from_secs(15));
                ticker.tick().await;
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            if let Err(err) = bg_state.run_scan().await {
                                eprintln!("planar background scan warning: {err}");
                            }
                        }
                        res = bg_stop.changed() => {
                            if res.is_err() || *bg_stop.borrow() {
                                break;
                            }
                        }
                    }
                }
            });

            let open_item = MenuItem::with_id(app, "open", "Open Planar", true, None::<&str>)?;
            let sync_item =
                MenuItem::with_id(app, "sync", "Sync Pending Sessions", true, None::<&str>)?;
            let scan_item = MenuItem::with_id(app, "scan", "Scan Logs Now", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            let quit_item =
                MenuItem::with_id(app, "quit", "Quit Planar", true, Some("CmdOrCtrl+Q"))?;

            let tray_menu = MenuBuilder::new(app)
                .item(&open_item)
                .item(&sync_item)
                .item(&scan_item)
                .item(&settings_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let scan_menu_state = state_setup;
            let quitting_menu = Arc::clone(&quitting_setup);
            let stop_tx_menu = stop_tx_setup;

            let mut tray = TrayIconBuilder::with_id("planar-tray")
                .tooltip("Planar — Lines × Time")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app_handle, event| match event.id.as_ref() {
                    "open" => focus_main_window(app_handle),
                    "sync" => {
                        focus_main_window(app_handle);
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ =
                                window.eval("window.__planarRunSync && window.__planarRunSync();");
                        }
                    }
                    "scan" => {
                        let scan_state = scan_menu_state.clone();
                        let handle = app_handle.clone();
                        focus_main_window(&handle);
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) = scan_state.run_scan().await {
                                eprintln!("planar manual tray scan warning: {err}");
                            }
                        });
                    }
                    "settings" => {
                        focus_main_window(app_handle);
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.eval(
                                "window.__planarOpenSettings && window.__planarOpenSettings();",
                            );
                        }
                    }
                    "quit" => {
                        quitting_menu.store(true, Ordering::SeqCst);
                        let _ = stop_tx_menu.send(true);
                        app_handle.exit(0);
                    }
                    _ => {}
                });

            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let win_close = window.clone();
                let quitting_win = quitting_setup;
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event
                        && !quitting_win.load(Ordering::SeqCst)
                    {
                        api.prevent_close();
                        let _ = win_close.hide();
                    }
                });

                window.navigate(ui_url)?;
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .context("failed to build Planar Tauri application")?;

    let quitting_run = quitting;
    let stop_tx_run = stop_tx;
    app.run(move |app_handle, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            focus_main_window(app_handle);
        }
        RunEvent::Exit => {
            quitting_run.store(true, Ordering::SeqCst);
            let _ = stop_tx_run.send(true);
        }
        _ => {}
    });

    Ok(())
}
