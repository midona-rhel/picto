#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

#[cfg(any(feature = "wayland", feature = "x11"))]
pub use linux::drop_data::{assemble as assemble_drop_data, wanted as wanted_drop_types};
pub use linux::{current_platform, linux_platform};
