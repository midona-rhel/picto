//! Files dragged out of a window to another application: the source side of
//! XDND (version 3 to 5). The window that started the drag owns
//! `XdndSelection` and, through the implicit grab of its button press, keeps
//! receiving the pointer's motion and release wherever the pointer is. Each
//! motion finds the XDND-aware window under the pointer and speaks to it
//! (`XdndEnter`, `XdndPosition`, `XdndLeave`); the release drops on a target
//! that accepted, which then asks for `text/uri-list` and answers
//! `XdndFinished`.
use std::path::PathBuf;

use gpui_util::ResultExt as _;
use x11rb::{
    connection::Connection as _,
    protocol::xproto::{
        self, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
        PropMode, SelectionNotifyEvent, SelectionRequestEvent,
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};

use super::XcbAtoms;

/// The highest XDND version spoken.
const VERSION: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Target {
    /// The XDND-aware window under the pointer.
    window: xproto::Window,
    /// Where its messages go: the window, or the proxy it names.
    deliver_to: xproto::Window,
    version: u32,
}

pub(crate) struct XdndSource {
    /// The window the drag started in, which owns `XdndSelection`.
    pub(crate) window: xproto::Window,
    uri_list: Vec<u8>,
    target: Option<Target>,
    /// A position was sent and its status has not arrived.
    waiting: bool,
    /// The latest position while one is waiting.
    pending: Option<(i16, i16, u32)>,
    accepted: bool,
    /// The drop was sent; the target's `XdndFinished` ends the drag.
    pub(crate) dropped: bool,
}

/// `text/uri-list` for `paths`: one `file:` address per line, each line
/// ended by CR LF.
pub fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
    let mut list = String::new();
    for path in paths {
        if let Ok(url) = url::Url::from_file_path(path) {
            list.push_str(url.as_str());
            list.push_str("\r\n");
        }
    }
    list.into_bytes()
}

fn send(connection: &XCBConnection, target: Target, kind: xproto::Atom, data: [u32; 5]) {
    let message = ClientMessageEvent {
        format: 32,
        window: target.window,
        type_: kind,
        data: ClientMessageData::from(data),
        sequence: 0,
        response_type: xproto::CLIENT_MESSAGE_EVENT,
    };
    connection
        .send_event(false, target.deliver_to, EventMask::NO_EVENT, message)
        .log_err();
}

fn property32(
    connection: &XCBConnection,
    window: xproto::Window,
    property: xproto::Atom,
    kind: impl Into<xproto::Atom>,
) -> Option<u32> {
    connection
        .get_property(false, window, property, kind, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
}

/// The XDND-aware window at root point (`x`, `y`): the first window, going
/// down from the root's child under the point, that has `XdndAware`, with
/// the proxy it names when that proxy names itself.
fn find_target(
    connection: &XCBConnection,
    atoms: &XcbAtoms,
    root: xproto::Window,
    x: i16,
    y: i16,
) -> Option<Target> {
    let mut parent = root;
    for _ in 0..16 {
        let child = connection
            .translate_coordinates(root, parent, x, y)
            .ok()?
            .reply()
            .ok()?
            .child;
        if child == x11rb::NONE {
            return None;
        }
        if let Some(version) = property32(connection, child, atoms.XdndAware, AtomEnum::ATOM) {
            if version < 3 {
                return None;
            }
            let deliver_to = property32(connection, child, atoms.XdndProxy, AtomEnum::WINDOW)
                .filter(|proxy| {
                    property32(connection, *proxy, atoms.XdndProxy, AtomEnum::WINDOW)
                        == Some(*proxy)
                })
                .unwrap_or(child);
            return Some(Target {
                window: child,
                deliver_to,
                version: version.min(VERSION),
            });
        }
        parent = child;
    }
    None
}

impl XdndSource {
    pub(crate) fn new(window: xproto::Window, paths: &[PathBuf]) -> Self {
        Self {
            window,
            uri_list: uri_list(paths),
            target: None,
            waiting: false,
            pending: None,
            accepted: false,
            dropped: false,
        }
    }

    fn position(&mut self, connection: &XCBConnection, atoms: &XcbAtoms, at: (i16, i16, u32)) {
        let Some(target) = self.target else {
            return;
        };
        let (x, y, time) = at;
        let point = ((x as u16 as u32) << 16) | y as u16 as u32;
        send(
            connection,
            target,
            atoms.XdndPosition,
            [self.window, 0, point, time, atoms.XdndActionCopy],
        );
        self.waiting = true;
    }

    /// The pointer moved to root point (`x`, `y`). A window `own` says is
    /// this process's is not a target: the app takes its own items back
    /// itself.
    pub(crate) fn motion(
        &mut self,
        connection: &XCBConnection,
        atoms: &XcbAtoms,
        root: xproto::Window,
        at: (i16, i16, u32),
        own: impl Fn(xproto::Window) -> bool,
    ) {
        if self.dropped {
            return;
        }
        let target =
            find_target(connection, atoms, root, at.0, at.1).filter(|target| !own(target.window));
        if target.map(|target| target.window) != self.target.map(|target| target.window) {
            if let Some(old) = self.target {
                send(connection, old, atoms.XdndLeave, [self.window, 0, 0, 0, 0]);
            }
            self.target = target;
            self.accepted = false;
            self.waiting = false;
            self.pending = None;
            if let Some(new) = target {
                send(
                    connection,
                    new,
                    atoms.XdndEnter,
                    [self.window, new.version << 24, atoms.TextUriList, 0, 0],
                );
            }
        }
        if self.target.is_some() {
            if self.waiting {
                self.pending = Some(at);
            } else {
                self.position(connection, atoms, at);
            }
        }
        connection.flush().log_err();
    }

    /// The target's `XdndStatus`: whether it would take the drop.
    pub(crate) fn status(&mut self, connection: &XCBConnection, atoms: &XcbAtoms, data: [u32; 5]) {
        if self.target.is_none_or(|target| target.window != data[0]) {
            return;
        }
        self.accepted = data[1] & 1 == 1;
        self.waiting = false;
        if let Some(at) = self.pending.take() {
            self.position(connection, atoms, at);
            connection.flush().log_err();
        }
    }

    /// The button was let go: a target that accepted gets the drop (and
    /// `true` is returned until its `XdndFinished`); any other is left.
    pub(crate) fn release(
        &mut self,
        connection: &XCBConnection,
        atoms: &XcbAtoms,
        time: u32,
    ) -> bool {
        let dropped = match self.target {
            Some(target) if self.accepted => {
                send(
                    connection,
                    target,
                    atoms.XdndDrop,
                    [self.window, 0, time, 0, 0],
                );
                true
            }
            Some(target) => {
                send(
                    connection,
                    target,
                    atoms.XdndLeave,
                    [self.window, 0, 0, 0, 0],
                );
                false
            }
            None => false,
        };
        self.dropped = dropped;
        connection.flush().log_err();
        dropped
    }

    /// Whether `data` is the `XdndFinished` of the target dropped on.
    pub(crate) fn is_finished_by(&self, data: [u32; 5]) -> bool {
        self.dropped && self.target.is_some_and(|target| target.window == data[0])
    }

    /// Answers a request for `XdndSelection`: its targets, or the files as
    /// `text/uri-list`; anything else is refused.
    pub(crate) fn answer(
        &self,
        connection: &XCBConnection,
        atoms: &XcbAtoms,
        request: &SelectionRequestEvent,
    ) {
        let property = if request.property == x11rb::NONE {
            request.target
        } else {
            request.property
        };
        let written = if request.target == atoms.TARGETS {
            connection
                .change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    AtomEnum::ATOM,
                    &[atoms.TARGETS, atoms.TextUriList],
                )
                .is_ok()
        } else if request.target == atoms.TextUriList {
            connection
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    atoms.TextUriList,
                    &self.uri_list,
                )
                .is_ok()
        } else {
            false
        };
        let notify = SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if written { property } else { x11rb::NONE },
        };
        connection
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
            .log_err();
        connection.flush().log_err();
    }
}
