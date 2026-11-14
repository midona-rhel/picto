//! Reading the accessibility tree a window builds, for tests and the
//! off-screen tool's `a11y` line: a [`Harness`] draws any element in a test
//! window with the tree switched on, and a [`Tree`] answers what assistive
//! technology would be told (each node's role, name, states and actions) and
//! writes it out as indented text.
use gpui::accesskit::{Action, ActionRequest, Node, NodeId, Role, Toggled, TreeUpdate};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, IntoElement, Pixels,
    Render, TestAppContext, Window,
};
use std::collections::HashMap;

/// One frame's tree.
pub struct Tree {
    nodes: HashMap<NodeId, Node>,
    root: NodeId,
    focus: NodeId,
}

/// A node of a [`Tree`].
#[derive(Clone, Copy)]
pub struct At<'a> {
    tree: &'a Tree,
    pub id: NodeId,
}

impl Tree {
    pub fn new(update: TreeUpdate) -> Self {
        let root = update.tree.as_ref().map(|tree| tree.root).unwrap_or(NodeId(0));
        Self {
            nodes: update.nodes.into_iter().collect(),
            root,
            focus: update.focus,
        }
    }

    pub fn root(&self) -> At<'_> {
        self.at(self.root)
    }

    pub fn at(&self, id: NodeId) -> At<'_> {
        At { tree: self, id }
    }

    /// The node assistive technology is told has the keyboard.
    pub fn focused(&self) -> At<'_> {
        self.at(self.focus)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Every node, parents before children, in drawing order.
    pub fn all(&self) -> Vec<At<'_>> {
        let mut out = Vec::new();
        self.root().walk(&mut |node, _| out.push(node));
        out
    }

    /// The nodes of `role`.
    pub fn with_role(&self, role: Role) -> Vec<At<'_>> {
        self.all().into_iter().filter(|node| node.role() == role).collect()
    }

    /// The first node of `role` named `name`, or a failure that shows the tree.
    pub fn find(&self, role: Role, name: &str) -> At<'_> {
        self.try_find(role, name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?} in\n{}", self.dump()))
    }

    pub fn try_find(&self, role: Role, name: &str) -> Option<At<'_>> {
        self.all()
            .into_iter()
            .find(|node| node.role() == role && node.name() == name)
    }

    /// The tree as indented text: a line per node, its role, name in quotes,
    /// value, and states.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.root().walk(&mut |node, depth| {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&node.line());
            out.push('\n');
        });
        out
    }
}

impl<'a> At<'a> {
    pub fn node(&self) -> &'a Node {
        &self.tree.nodes[&self.id]
    }

    pub fn role(&self) -> Role {
        self.node().role()
    }

    pub fn children(&self) -> Vec<At<'a>> {
        let tree = self.tree;
        self.node()
            .children()
            .iter()
            .filter(|id| tree.nodes.contains_key(id))
            .map(|id| tree.at(*id))
            .collect()
    }

    fn walk(&self, visit: &mut impl FnMut(At<'a>, usize)) {
        fn go<'a>(node: At<'a>, depth: usize, visit: &mut impl FnMut(At<'a>, usize)) {
            visit(node, depth);
            for child in node.children() {
                go(child, depth + 1, visit);
            }
        }
        go(*self, 0, visit);
    }

    /// The descendants of this node, parents first.
    pub fn descendants(&self) -> Vec<At<'a>> {
        let mut out = Vec::new();
        self.walk(&mut |node, depth| {
            if depth > 0 {
                out.push(node)
            }
        });
        out
    }

    /// The first descendant of `role` named `name`.
    pub fn find(&self, role: Role, name: &str) -> At<'a> {
        self.descendants()
            .into_iter()
            .find(|node| node.role() == role && node.name() == name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?} under\n{}", self.tree.dump()))
    }

    /// The name assistive technology reads, as the platforms compute it from
    /// AccessKit: the label, else the text of the labels inside it.
    pub fn name(&self) -> String {
        if let Some(label) = self.node().label() {
            return label.to_owned();
        }
        if self.role() == Role::Label {
            return self.node().value().unwrap_or_default().to_owned();
        }
        let mut words = Vec::new();
        for node in self.descendants() {
            if node.role() == Role::Label
                && let Some(text) = node.node().value()
            {
                words.push(text.to_owned());
            }
        }
        words.join(" ")
    }

    pub fn value(&self) -> Option<&'a str> {
        self.node().value()
    }

    pub fn toggled(&self) -> Option<Toggled> {
        self.node().toggled()
    }

    pub fn selected(&self) -> Option<bool> {
        self.node().is_selected()
    }

    pub fn expanded(&self) -> Option<bool> {
        self.node().is_expanded()
    }

    pub fn disabled(&self) -> bool {
        self.node().is_disabled()
    }

    pub fn supports(&self, action: Action) -> bool {
        self.node().supports_action(action)
    }

    pub fn bounds(&self) -> Option<gpui::accesskit::Rect> {
        self.node().bounds()
    }

    fn line(&self) -> String {
        let node = self.node();
        let mut line = format!("{:?}", node.role());
        let name = self.name();
        if !name.is_empty() && self.role() != Role::Label {
            line.push_str(&format!(" {name:?}"));
        }
        if let Some(value) = node.value() {
            line.push_str(&format!(" value={value:?}"));
        }
        if let Some(value) = node.numeric_value() {
            line.push_str(&format!(" value={value}"));
        }
        if let (Some(min), Some(max)) = (node.min_numeric_value(), node.max_numeric_value()) {
            line.push_str(&format!(" range={min}..{max}"));
        }
        if let Some(text) = node.placeholder() {
            line.push_str(&format!(" placeholder={text:?}"));
        }
        if let Some(text) = node.description() {
            line.push_str(&format!(" description={text:?}"));
        }
        let mut states = Vec::new();
        match node.toggled() {
            Some(Toggled::True) => states.push("checked".to_owned()),
            Some(Toggled::False) => states.push("unchecked".to_owned()),
            Some(Toggled::Mixed) => states.push("mixed".to_owned()),
            None => {}
        }
        match node.is_selected() {
            Some(true) => states.push("selected".to_owned()),
            Some(false) => states.push("unselected".to_owned()),
            None => {}
        }
        match node.is_expanded() {
            Some(true) => states.push("expanded".to_owned()),
            Some(false) => states.push("collapsed".to_owned()),
            None => {}
        }
        if node.is_disabled() {
            states.push("disabled".to_owned());
        }
        if node.is_read_only() {
            states.push("read-only".to_owned());
        }
        if node.is_modal() {
            states.push("modal".to_owned());
        }
        if node.is_busy() {
            states.push("busy".to_owned());
        }
        if node.is_multiselectable() {
            states.push("multiselectable".to_owned());
        }
        if let Some(live) = node.live() {
            states.push(format!("live={live:?}").to_lowercase());
        }
        if let Some(level) = node.level() {
            states.push(format!("level={level}"));
        }
        if let (Some(position), Some(size)) = (node.position_in_set(), node.size_of_set()) {
            states.push(format!("{position} of {size}"));
        }
        if let Some(count) = node.row_count() {
            states.push(format!("rows={count}"));
        }
        if let Some(text) = node.keyboard_shortcut() {
            states.push(format!("keys={text}"));
        }
        if self.tree.focus == self.id && self.id != self.tree.root {
            states.push("focused".to_owned());
        }
        if !states.is_empty() {
            line.push_str(&format!(" [{}]", states.join(", ")));
        }
        line
    }
}

/// A test window that draws one element with the tree switched on.
pub struct Harness {
    pub cx: TestAppContext,
    pub window: AnyWindowHandle,
}

type Draw = Box<dyn Fn(&mut Window, &mut App) -> AnyElement>;

struct Host(Draw);

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::{ParentElement as _, Styled as _};
        gpui::div().size_full().child((self.0)(window, cx))
    }
}

impl Harness {
    /// A window drawing what `draw` returns each frame, with the dark theme
    /// and the shared controls' keys, as the app prepares itself.
    pub fn new<E: IntoElement + 'static>(
        draw: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            cx.set_global(crate::theme::DARK);
            crate::ui::focus::bind(cx);
        });
        let draw: Draw = Box::new(move |window, cx| draw(window, cx).into_any_element());
        let window: AnyWindowHandle = cx
            .add_window(move |window, _| {
                window.set_a11y_active_for_test(true);
                Host(draw)
            })
            .into();
        Self { cx, window }
    }

    /// Lets the window settle and draws a frame: the tree it built.
    pub fn tree(&mut self) -> Tree {
        self.cx.run_until_parked();
        let update = self
            .cx
            .update_window(self.window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                window.last_a11y_tree_update()
            })
            .unwrap()
            .expect("the window built no tree");
        Tree::new(update)
    }

    /// Elements in the last frame that take a click but have no role.
    pub fn unexposed(&mut self) -> Vec<(String, Bounds<Pixels>)> {
        self.cx
            .update_window(self.window, |_, window, _| window.a11y_unexposed_clickables())
            .unwrap()
    }

    /// Asks for `action` on `node`, as assistive technology does.
    pub fn act(&mut self, node: NodeId, action: Action) {
        self.cx
            .update_window(self.window, |_, window, cx| {
                window.dispatch_a11y_action_for_test(
                    ActionRequest {
                        action,
                        target_tree: gpui::accesskit::TreeId::ROOT,
                        target_node: node,
                        data: None,
                    },
                    cx,
                )
            })
            .unwrap();
        self.cx.run_until_parked();
    }
}
