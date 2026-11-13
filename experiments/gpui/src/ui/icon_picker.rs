//! The icon picker (`IconPicker.tsx`) in its compact presentation, the one
//! Picto's icon surfaces use: the default-folder tile, then the curated
//! icons, ten to a row in a 252×300 box. Choosing a tile tells the owner and
//! keeps the picker open with the new choice marked.
use crate::theme;
use gpui::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

/// `CURATED_ICONS` in `iconRegistry.ts`: the name stored on a folder, which
/// is also the glyph's file in `assets/`, and its label.
pub const CURATED_ICONS: [(&str, &str); 115] = [
    ("IconPhoto", "Photo"),
    ("IconVideo", "Video"),
    ("IconMusic", "Music"),
    ("IconCamera", "Camera"),
    ("IconPlayerPlay", "Play"),
    ("IconMicrophone", "Microphone"),
    ("IconHeadphones", "Headphones"),
    ("IconVolume", "Volume"),
    ("IconScreenShare", "Screen"),
    ("IconBrandYoutube", "YouTube"),
    ("IconFile", "File"),
    ("IconFileText", "Text File"),
    ("IconFolder", "Folder"),
    ("IconFolderOpen", "Open Folder"),
    ("IconArchive", "Archive"),
    ("IconClipboard", "Clipboard"),
    ("IconNote", "Note"),
    ("IconBook", "Book"),
    ("IconNotebook", "Notebook"),
    ("IconFileZip", "Zip"),
    ("IconSquare", "Square"),
    ("IconCircle", "Circle"),
    ("IconTriangle", "Triangle"),
    ("IconHexagon", "Hexagon"),
    ("IconDiamond", "Diamond"),
    ("IconStar", "Star"),
    ("IconHeart", "Heart"),
    ("IconSparkles", "Sparkles"),
    ("IconHome", "Home"),
    ("IconBuildingSkyscraper", "Building"),
    ("IconCar", "Car"),
    ("IconPlane", "Plane"),
    ("IconRocket", "Rocket"),
    ("IconAnchor", "Anchor"),
    ("IconCompass", "Compass"),
    ("IconMap", "Map"),
    ("IconGlobe", "Globe"),
    ("IconFlag", "Flag"),
    ("IconSun", "Sun"),
    ("IconMoon", "Moon"),
    ("IconCloud", "Cloud"),
    ("IconSnowflake", "Snowflake"),
    ("IconDroplet", "Droplet"),
    ("IconFlame", "Flame"),
    ("IconLeaf", "Leaf"),
    ("IconTree", "Tree"),
    ("IconMountain", "Mountain"),
    ("IconWind", "Wind"),
    ("IconDog", "Dog"),
    ("IconCat", "Cat"),
    ("IconFish", "Fish"),
    ("IconBug", "Bug"),
    ("IconFeather", "Feather"),
    ("IconPaw", "Paw"),
    ("IconUser", "User"),
    ("IconUsers", "Users"),
    ("IconUserCircle", "User Circle"),
    ("IconMoodSmile", "Smile"),
    ("IconMoodHappy", "Happy"),
    ("IconFriends", "Friends"),
    ("IconMan", "Man"),
    ("IconWoman", "Woman"),
    ("IconAccessible", "Accessible"),
    ("IconMail", "Mail"),
    ("IconMessage", "Message"),
    ("IconBell", "Bell"),
    ("IconPhone", "Phone"),
    ("IconBrandTwitter", "Twitter"),
    ("IconBrandInstagram", "Instagram"),
    ("IconSend", "Send"),
    ("IconAt", "At"),
    ("IconHash", "Hash"),
    ("IconBrandDiscord", "Discord"),
    ("IconPalette", "Palette"),
    ("IconBrush", "Brush"),
    ("IconPencil", "Pencil"),
    ("IconHighlight", "Highlight"),
    ("IconScissors", "Scissors"),
    ("IconRuler", "Ruler"),
    ("IconColorSwatch", "Swatch"),
    ("IconWand", "Wand"),
    ("IconSpray", "Spray"),
    ("IconCode", "Code"),
    ("IconTerminal", "Terminal"),
    ("IconDatabase", "Database"),
    ("IconServer", "Server"),
    ("IconCpu", "CPU"),
    ("IconWifi", "Wifi"),
    ("IconBluetooth", "Bluetooth"),
    ("IconDeviceDesktop", "Desktop"),
    ("IconDeviceMobile", "Mobile"),
    ("IconBrandGithub", "GitHub"),
    ("IconTag", "Tag"),
    ("IconBookmark", "Bookmark"),
    ("IconPin", "Pin"),
    ("IconLock", "Lock"),
    ("IconShield", "Shield"),
    ("IconEye", "Eye"),
    ("IconClock", "Clock"),
    ("IconCalendar", "Calendar"),
    ("IconGift", "Gift"),
    ("IconTrophy", "Trophy"),
    ("IconCrown", "Crown"),
    ("IconBolt", "Bolt"),
    ("IconTarget", "Target"),
    ("IconPuzzle", "Puzzle"),
    ("IconDice", "Dice"),
    ("IconChess", "Chess"),
    ("IconAward", "Award"),
    ("IconCoffee", "Coffee"),
    ("IconPizza", "Pizza"),
    ("IconApple", "Apple"),
    ("IconPersonSimple", "Person"),
    ("IconPersonMale", "Male"),
    ("IconPersonFemale", "Female"),
];

/// `.compact`: the picker's box, padding and tiles.
pub const WIDTH: f32 = 252.;
pub const HEIGHT: f32 = 300.;
const PADDING: f32 = 6.;
const TILE: f32 = 24.;
const GLYPH: f32 = 20.;
const COLUMNS: usize = 10;

type Handler = Rc<dyn Fn(Option<String>, &mut Window, &mut App) + 'static>;

pub struct IconPicker {
    value: Option<String>,
    default_label: SharedString,
    on_change: Handler,
}

impl IconPicker {
    /// A picker showing `value` chosen (`None` is the default icon).
    pub fn new(
        value: Option<String>,
        on_change: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            value,
            default_label: "Use default folder icon".into(),
            on_change: Rc::new(on_change),
        }
    }

    fn pick(&mut self, value: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.value = value.clone();
        cx.notify();
        (self.on_change.clone())(value, window, cx);
    }

    fn tile(
        &self,
        index: usize,
        glyph: &'static str,
        label: SharedString,
        value: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let selected = self.value.as_deref() == value;
        let mut glyph = svg().path(glyph).size(px(GLYPH)).text_color(if selected {
            theme.text()
        } else {
            theme.text_secondary()
        });
        if !selected {
            glyph = glyph.group_hover("icon-tile", move |style| style.text_color(theme.text()));
        }
        let tile = div()
            .id(("icon-tile", index))
            .group("icon-tile")
            .size(px(TILE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .when(selected, |tile| tile.bg(theme.surface_active()))
            .when(!selected, |tile| {
                tile.hover(move |style| style.bg(theme.hover_overlay()))
            })
            .child(glyph)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.pick(value.map(str::to_owned), window, cx)
            }));
        super::tooltip::attach(
            tile,
            ("icon-tile-tip", index),
            super::tip(label),
            TILE,
            window,
            cx,
        )
    }
}

impl Render for IconPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut grid = div().flex().flex_wrap().w(px(TILE * COLUMNS as f32));
        let label = self.default_label.clone();
        grid = grid.child(self.tile(0, "folder", label, None, window, cx));
        for (index, (name, label)) in CURATED_ICONS.iter().enumerate() {
            grid = grid.child(self.tile(index + 1, name, (*label).into(), Some(name), window, cx));
        }
        div()
            .id("icon-picker")
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .overflow_hidden()
            .p(px(PADDING))
            // A press inside the picker is not a press outside the menu.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(grid)
    }
}
