#![warn(missing_docs)]
//! **Smithay: the wayland compositor smithy**
//!
//! Most entry points in the modules can take an optionnal `slog::Logger` as argument
//! that will be used as a drain for logging. If `None` is provided, they'll log to `slog-stdlog`.

#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]

extern crate nix;
#[macro_use]
extern crate rental;
extern crate tempfile;
extern crate wayland_protocols;
#[macro_use]
extern crate wayland_server;
extern crate xkbcommon;

#[cfg(feature = "backend_libinput")]
extern crate input;
#[cfg(feature = "backend_winit")]
extern crate wayland_client;
#[cfg(feature = "backend_winit")]
extern crate winit;

extern crate libloading;

#[cfg(feature = "renderer_glium")]
extern crate glium;

#[macro_use]
extern crate slog;
extern crate slog_stdlog;

pub mod backend;
pub mod compositor;
pub mod shm;
pub mod keyboard;
pub mod shell;

fn slog_or_stdlog<L>(logger: L) -> ::slog::Logger
where
    L: Into<Option<::slog::Logger>>,
{
    use slog::Drain;
    logger
        .into()
        .unwrap_or_else(|| ::slog::Logger::root(::slog_stdlog::StdLog.fuse(), o!()))
}
