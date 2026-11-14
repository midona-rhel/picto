//! What export shows: the Export dialog (`ExportModal.tsx`) and the Picto
//! Pack dialog (`PictoPackModal.tsx`), both on the shared dialog with
//! `GlassModal.module.css`'s form layout.
use super::{Convert, Dialog, Exports, Format, Pack, PackMode, PackSource, format_bytes};
use crate::theme::Text as _;
use crate::ui::action_button::Look as ButtonLook;
use crate::ui::field::Look as FieldLook;
use crate::ui::modal::{
    form_field as field, form_label as label, form_row as row, form_stack as stack,
};
use crate::{theme, ui};
use gpui::*;

/// Runs `action` on the exports from a control's handler.
fn on(
    this: &WeakEntity<Exports>,
    action: impl Fn(&mut Exports, &mut Context<Exports>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = this.clone();
    move |_, cx| {
        let _ = this.update(cx, |exports, cx| action(exports, cx));
    }
}

fn on_click(
    this: &WeakEntity<Exports>,
    action: impl Fn(&mut Exports, &mut Context<Exports>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handler = on(this, action);
    move |_, window, cx| handler(window, cx)
}

/// `.separator`: a 1px rule with 12px above and below.
fn separator(theme: &theme::Theme) -> Div {
    div()
        .h(px(1.))
        .my(px(12.))
        .flex_none()
        .bg(theme.border_secondary())
}

/// "Export 3 Files", with the count as production writes it, ungrouped.
pub(super) fn convert_title(count: u64) -> String {
    format!("Export {count} File{}", if count == 1 { "" } else { "s" })
}

impl Render for Exports {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div().absolute().inset_0();
        if let Some(layer) = self.dialog_layer(window, cx) {
            root = root.child(layer);
        }
        root
    }
}

impl Exports {
    fn dialog_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let layer = self.dialog.layer(window, cx)?;
        let theme = theme::theme(cx);
        let this = cx.entity().downgrade();
        let close = on(&this, |exports, cx| exports.dismiss(cx));
        let modal = match &layer.content {
            Dialog::Convert(convert) => {
                Self::convert_dialog(convert, &theme, &this, layer.presence, cx)
            }
            Dialog::Pack(pack) => Self::pack_dialog(pack, &theme, &this, layer.presence),
        };
        Some(modal.focus(&layer.focus).on_close(close).into_any_element())
    }

    /// `ExportModal`: where, in which format and quality, and at what size.
    fn convert_dialog(
        convert: &Convert,
        theme: &theme::Theme,
        this: &WeakEntity<Exports>,
        presence: ui::modal::Presence,
        cx: &App,
    ) -> ui::modal::Modal {
        let format = convert.format;
        let destination = row()
            .child(ui::field::read_only_modal(
                "export-folder",
                convert
                    .output_dir
                    .as_ref()
                    .map(|dir| SharedString::from(dir.to_string_lossy().into_owned())),
                "Choose export folder...",
                cx,
            ))
            .child(
                ui::action_button("export-browse", ButtonLook::Standard)
                    .label("Browse")
                    .on_click(on_click(this, |exports, cx| exports.browse(cx))),
            );
        let choices = Format::ALL
            .into_iter()
            .map(|format| ui::choice(format.value(), format.label()))
            .collect();
        let select = {
            let this = this.clone();
            ui::select("export-format", format.value(), choices)
                .width(180.)
                .on_change(move |value, _, cx| {
                    let _ = this.update(cx, |exports, cx| exports.set_format(value, cx));
                })
        };
        let mut body = stack()
            .child(field(theme, "Destination", destination))
            .child(field(theme, "Format", div().flex().child(select)));
        if format.has_quality() {
            let this = this.clone();
            body = body.child(field(
                theme,
                format!("Quality — {}%", convert.quality),
                ui::slider("export-quality", convert.quality as f64, 1., 100., 1.)
                    .dialog()
                    .on_change(move |value, cx| {
                        let _ = this.update(cx, |exports, cx| exports.set_quality(value, cx));
                    }),
            ));
        }
        if format.resizes() {
            let size_field = |id: &'static str, input| {
                div()
                    .w(px(100.))
                    .flex_none()
                    .child(ui::field(id, input, FieldLook::Modal))
            };
            body = body
                .child(separator(theme))
                .child(field(
                    theme,
                    "Resize (optional)",
                    row()
                        .child(size_field("export-width", &convert.width))
                        // `.inlineLabel`
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .text(theme::TEXT_SM)
                                .text_color(theme.text_secondary())
                                .child("×"),
                        )
                        .child(size_field("export-height", &convert.height)),
                ))
                .child(
                    row()
                        .justify_between()
                        .child(label(theme, "Keep Aspect Ratio"))
                        .child(
                            ui::toggle("export-keep-aspect", convert.keep_aspect)
                                .on_toggle(on(this, |exports, cx| exports.toggle_aspect(cx))),
                        ),
                );
        }
        ui::modal("export-dialog", convert_title(convert.count), presence)
            .size(ui::modal::Size::Medium)
            .child(body)
            .footer(
                ui::action_button("export-cancel", ButtonLook::Standard)
                    .label("Cancel")
                    .on_click(on_click(this, |exports, cx| {
                        exports.dialog.close(cx);
                    })),
            )
            .footer(
                ui::action_button("export-go", ButtonLook::Primary)
                    .label("Export")
                    .disabled(convert.output_dir.is_none())
                    .on_click(on_click(this, |exports, cx| exports.convert(cx))),
            )
    }

    /// `PictoPackModal`: what a pack keeps and, for one being imported,
    /// what it holds.
    fn pack_dialog(
        pack: &Pack,
        theme: &theme::Theme,
        this: &WeakEntity<Exports>,
        presence: ui::modal::Presence,
    ) -> ui::modal::Modal {
        let importing = matches!(pack.mode, PackMode::Import { .. });
        let heading = match &pack.mode {
            PackMode::Import { summary, .. } => summary.name.clone(),
            PackMode::Export { .. } => "Portable library package".into(),
        };
        let mut notes = div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text_secondary())
                    .child(heading),
            )
            .child(label(
                theme,
                "Picto Packs keep original media, collections, names, notes, dates, ratings, tags, source links, and included folder definitions.",
            ))
            .child(label(
                theme,
                "Subscriptions, provider history, authentication, and folder watch paths are never included.",
            ));
        if let PackMode::Export {
            source: PackSource::SmartFolder(_),
            ..
        } = &pack.mode
        {
            notes = notes.child(label(
                theme,
                "Smart-folder exports contain only the current matching items. The smart-folder rule and container are not included.",
            ));
        }
        // The tile is 48 wide and 53 tall in production: its icon is an
        // inline SVG, which sits on a text line and leaves the line's
        // descender below it.
        let glyph = div()
            .flex_none()
            .h(px(53.))
            .p(px(10.))
            .rounded(px(12.))
            .bg(rgba(theme::PACK_GLYPH_TILE))
            .child(ui::icon(
                if importing {
                    "pack-import"
                } else {
                    "pack-export"
                },
                28.,
                theme.text_secondary(),
            ));
        // `.rowSpread`: a name and its value in bold.
        let fact = |name: &'static str, value: String| {
            row()
                .justify_between()
                .child(div().child(name))
                .child(div().font_weight(FontWeight::BOLD).child(value))
        };
        let facts = match &pack.mode {
            PackMode::Import { summary, .. } => stack()
                .child(fact("Items", summary.root_count.to_string()))
                .child(fact("Media files", summary.media_count.to_string()))
                .child(fact("Folders", summary.folder_count.to_string()))
                .child(fact(
                    "Smart folders",
                    summary.smart_folder_count.to_string(),
                ))
                .child(fact("Original size", format_bytes(summary.total_bytes))),
            PackMode::Export { count, .. } => fact("Selected items", count.to_string()),
        };
        let busy = pack.busy;
        ui::modal(
            "pack-dialog",
            if importing {
                "Import Picto Pack"
            } else {
                "Export Picto Pack"
            },
            presence,
        )
        .size(ui::modal::Size::Medium)
        .child(
            stack()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(14.))
                        .child(glyph)
                        .child(notes),
                )
                .child(separator(theme))
                .child(facts),
        )
        .footer(
            ui::action_button("pack-cancel", ButtonLook::Standard)
                .label("Cancel")
                .disabled(busy)
                .on_click(on_click(this, |exports, cx| {
                    exports.dialog.close(cx);
                })),
        )
        .footer(
            ui::action_button("pack-go", ButtonLook::Primary)
                .label(if busy {
                    "Working..."
                } else if importing {
                    "Import"
                } else {
                    "Choose Destination..."
                })
                .disabled(busy)
                .on_click(on_click(this, |exports, cx| exports.run_pack(cx))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::convert_title;

    #[test]
    fn the_export_dialog_counts_its_files() {
        assert_eq!(convert_title(1), "Export 1 File");
        assert_eq!(convert_title(1200), "Export 1200 Files");
    }
}
