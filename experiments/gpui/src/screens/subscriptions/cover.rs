//! A subscription's cover (`SubscriptionCoverDialog`:
//! `subscriptions.cover.candidates`, `subscriptions.cover.set`) in the shared
//! cover dialog, `ui::cover_dialog`.
use super::dialogs::Dialog;
use super::{SubscriptionsScreen, data, requests};
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
use crate::ui::cover_dialog::{Candidate, Cover, CoverHost, Target, add_page};
use gpui::*;
use picto_core::subscription_catalog::SubscriptionCoverCandidatePage;
use serde_json::Value;
use std::rc::Rc;

const INSTRUCTIONS: &str =
    crate::i18n::en("Select one of the images downloaded that has been added to the library.");
const EMPTY: &str = crate::i18n::en("No images have been added yet.");

impl SubscriptionsScreen {
    /// The menu's Set Cover Photo….
    pub(super) fn open_cover(&mut self, id: i64, cx: &mut Context<Self>) {
        self.show_dialog(
            Dialog::Cover(Box::new(Cover::new(
                Target::Subscription(id),
                None,
                INSTRUCTIONS,
                EMPTY,
            ))),
            None,
            cx,
        );
        self.load_candidates(id, None, cx);
    }

    /// A page of candidates, after `cursor`, for the dialog of `id`.
    fn load_candidates(&mut self, id: i64, cursor: Option<Value>, cx: &mut Context<Self>) {
        let more = cursor.is_some();
        let answer = self.send(requests::cover_candidates(id, cursor), cx);
        let blobs = self.session.read(cx).blobs().to_path_buf();
        cx.spawn(async move |this, cx| {
            let page = answer
                .await
                .unwrap_or_else(|_| Err("The application has stopped".to_owned()))
                .and_then(|page| {
                    serde_json::from_value::<SubscriptionCoverCandidatePage>(page)
                        .map_err(|error| error.to_string())
                });
            // Where each thumbnail is, found off the thread that draws.
            let page = match page {
                Ok(page) => Ok(cx
                    .background_spawn(async move {
                        let next = page
                            .next_cursor
                            .and_then(|cursor| serde_json::to_value(cursor).ok());
                        let candidates = page
                            .candidates
                            .into_iter()
                            .map(|candidate| Candidate {
                                media_item: candidate.media_item_id,
                                size: (
                                    candidate
                                        .pixel_width
                                        .and_then(|w| u32::try_from(w).ok())
                                        .unwrap_or(1),
                                    candidate
                                        .pixel_height
                                        .and_then(|h| u32::try_from(h).ok())
                                        .unwrap_or(1),
                                ),
                                thumbnail: data::thumbnail(&blobs, &candidate.file_hash),
                                hash: candidate.file_hash,
                            })
                            .collect::<Vec<_>>();
                        (candidates, next)
                    })
                    .await),
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |this, cx| {
                let Some(cover) = this
                    .cover()
                    .filter(|cover| cover.target == Target::Subscription(id))
                else {
                    return;
                };
                let again = match page {
                    Ok((candidates, next)) => add_page(cover, candidates, next),
                    Err(error) => {
                        cover.loading = false;
                        cover.loading_more = false;
                        if more {
                            crate::ui::notify::error(
                                t("Unable to load more subscription media"),
                                error,
                                cx,
                            );
                        } else {
                            cover.candidates = Rc::default();
                            crate::ui::notify::error(
                                t("Unable to load subscription media"),
                                error,
                                cx,
                            );
                        }
                        None
                    }
                };
                if let Some(cursor) = again {
                    this.load_candidates(id, Some(cursor), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl CoverHost for SubscriptionsScreen {
    fn cover(&mut self) -> Option<&mut Cover> {
        match self.dialog.content_mut() {
            Some(Dialog::Cover(cover)) => Some(cover),
            _ => None,
        }
    }

    fn cover_busy(&self) -> bool {
        self.busy()
    }

    fn cover_thumbnails(&self) -> Entity<Thumbnails> {
        self.covers.clone()
    }

    fn load_more_candidates(&mut self, cx: &mut Context<Self>) {
        let Some(cover) = self.cover() else {
            return;
        };
        if cover.loading_more || cover.loading {
            return;
        }
        let Some(cursor) = cover.next.clone() else {
            return;
        };
        let Target::Subscription(id) = cover.target else {
            return;
        };
        cover.loading_more = true;
        self.load_candidates(id, Some(cursor), cx);
        cx.notify();
    }

    fn save_cover(&mut self, cx: &mut Context<Self>) {
        let Some(cover) = self.cover() else {
            return;
        };
        let (Some(selected), Target::Subscription(id)) = (cover.selected.as_ref(), cover.target)
        else {
            return;
        };
        let request = requests::set_cover(id, selected.media_item, cover.crop);
        if self.busy() {
            return;
        }
        self.act(vec![request], cx, |this, answer, cx| {
            if answer.is_ok() && this.cover().is_some() {
                this.dialog.close(cx);
            }
        });
    }

    fn dismiss_cover(&mut self, cx: &mut Context<Self>) {
        self.dialog.dismiss(cx);
    }
}
