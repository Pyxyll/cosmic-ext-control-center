//! Media controls via D-Bus MPRIS (`playerctld`), ported from the
//! cosmic-mediaplayer-applet: album art, title/artist, progress, transport.
//! Adapts to width: 1col = play/pause, 2col = title + transport, 3–4col adds
//! album art + a progress bar.

use super::mpris::{self, MprisState};
use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize, cols_for_width};
use crate::theme;
use cosmic::app::Task;
use cosmic::iced::widget::{Stack, image::Handle};
use cosmic::iced::{Alignment, ContentFit, Length};
use cosmic::prelude::*;
use cosmic::widget;
use zbus::blocking::Connection;

/// The two media tile looks. `Cosmic` follows the design system (a plain card);
/// `Framed` is the custom blurred album-art backdrop. Picked at add time — each
/// is its own palette entry / module id.
#[derive(Clone, Copy, PartialEq)]
enum Style {
    Cosmic,
    Framed,
}

pub struct MediaModule {
    desc: ModuleDescriptor,
    style: Style,
    conn: Option<Connection>,
    state: MprisState,
    /// (art file path, sharp handle, blurred-backdrop handle) — cached so
    /// identical art isn't reloaded/reblurred.
    art: Option<(String, Handle, Handle)>,
    /// While the user drags the seek handle: the 0..1 position they're at (so
    /// the bar follows the drag and isn't fought by the poll).
    scrubbing: Option<f32>,
}

fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Decode artwork into (sharp, blurred-backdrop) handles. The backdrop is
/// downscaled + Gaussian-blurred with rounded corners baked into its alpha
/// (iced won't round a background image), ported from the media applet.
fn load_art(path: &str) -> Option<(Handle, Handle)> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let sharp = Handle::from_rgba(w, h, img.clone().into_raw());

    let small = image::imageops::resize(&img, 320, 120, image::imageops::FilterType::Triangle);
    let mut blurred = image::imageops::blur(&small, 6.0);
    apply_rounded_mask(&mut blurred, 16.0);
    let (bw, bh) = (blurred.width(), blurred.height());
    let backdrop = Handle::from_rgba(bw, bh, blurred.into_raw());

    Some((sharp, backdrop))
}

/// Bake a rounded-rect alpha mask into an RGBA image (corners → transparent),
/// so a stretched background image gets rounded corners without iced's
/// border_radius path.
fn apply_rounded_mask(img: &mut image::RgbaImage, radius: f32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = if fx < radius {
                radius - fx
            } else if fx > w - radius {
                fx - (w - radius)
            } else {
                0.0
            };
            let dy = if fy < radius {
                radius - fy
            } else if fy > h - radius {
                fy - (h - radius)
            } else {
                0.0
            };
            let dist = (dx * dx + dy * dy).sqrt();
            let f = if dist <= radius - 0.5 {
                1.0
            } else if dist >= radius + 0.5 {
                0.0
            } else {
                radius + 0.5 - dist
            };
            let p = img.get_pixel_mut(x, y);
            p[3] = (p[3] as f32 * f) as u8;
        }
    }
}

/// A snapshot fetched off the UI thread: player state plus, when the track's
/// artwork changed, the decoded (sharp, blurred) handles. The D-Bus query and
/// the image decode/blur are the slow parts this keeps off the UI thread.
#[derive(Default, Clone)]
struct MediaData {
    state: MprisState,
    art: Option<(String, Handle, Handle)>,
}

fn fetch(conn: Option<Connection>, cur_art_path: Option<String>) -> MediaData {
    let mut d = MediaData::default();
    if let Some(c) = &conn {
        if let Ok(s) = mpris::fetch_state(c) {
            d.state = s;
        }
    }
    if let Some(p) = d.state.art_path.clone() {
        if cur_art_path.as_deref() != Some(p.as_str()) {
            if let Some((sharp, blurred)) = load_art(&p) {
                d.art = Some((p, sharp, blurred));
            }
        }
    }
    d
}

impl MediaModule {
    /// COSMIC-styled media tile (plain card) — the default.
    pub fn new() -> Self {
        Self::with_style(Style::Cosmic, "builtin.media", "Media")
    }

    /// The custom blurred album-art backdrop look.
    pub fn new_framed() -> Self {
        Self::with_style(Style::Framed, "builtin.media_art", "Media (album art)")
    }

    fn with_style(style: Style, id: &str, name: &str) -> Self {
        // No D-Bus connect / art decode in the editor preview.
        let conn = if super::preview() {
            None
        } else {
            mpris::connect().ok()
        };
        let mut m = Self {
            desc: ModuleDescriptor {
                id: id.into(),
                name: name.into(),
                icon: "applications-multimedia-symbolic".into(),
                size: TileSize::Large,
                resizable: true,
            },
            style,
            conn,
            state: MprisState::default(),
            art: None,
            scrubbing: None,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        let cur = self.art.as_ref().map(|(p, ..)| p.clone());
        let d = fetch(self.conn.clone(), cur);
        self.set(d);
    }

    fn set(&mut self, d: MediaData) {
        self.state = d.state;
        // Only replace art when the fetch loaded new artwork; otherwise keep the
        // last (a player can briefly drop artUrl mid-transition).
        if let Some(art) = d.art {
            self.art = Some(art);
        }
    }

    /// Transport row. The icon-button presets jump 16→32→40px, so we use
    /// `button::custom` with an explicitly-sized icon for a modest bump.
    /// `full` = prev/play/next; otherwise just play/pause (compact 2col tile).
    fn transport(&self, id: InstanceId, edit: bool, full: bool) -> Element<'_, Message> {
        let mk = |name: &str, control: &'static str, enabled: bool, px: u16| -> Element<'_, Message> {
            let mut b = widget::button::custom(widget::icon::from_name(name).size(px))
                .class(cosmic::theme::Button::Icon)
                .padding(6);
            if enabled && !edit {
                b = b.on_press(Message::Control(id, control.into(), ControlValue::Trigger));
            }
            b.into()
        };
        let play_icon = if self.state.playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        if !full {
            return mk(play_icon, "playpause", true, 28);
        }
        widget::Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(mk("media-skip-backward-symbolic", "prev", self.state.can_prev, 22))
            .push(mk(play_icon, "playpause", true, 28))
            .push(mk("media-skip-forward-symbolic", "next", self.state.can_next, 22))
            .into()
    }

    /// Rounded album-art thumbnail (or a fallback icon) at the given size.
    fn art_thumb(&self, size: f32) -> Element<'_, Message> {
        match &self.art {
            Some((_, sharp, _)) => widget::image(sharp.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(ContentFit::Cover)
                .border_radius([12.0; 4])
                .into(),
            None => widget::container(
                widget::icon::from_name(self.desc.icon.as_str()).size((size * 0.5) as u16),
            )
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
        }
    }

    /// Title + artist text block, ellipsized to the column width.
    fn info_text(&self, cols: u32) -> Element<'_, Message> {
        let title = if self.state.title.is_empty() {
            "Unknown".to_string()
        } else {
            ellipsize(&self.state.title, if cols >= 3 { 26 } else { 16 })
        };
        let subtitle = ellipsize(&self.state.artist, if cols >= 3 { 30 } else { 18 });
        widget::Column::new()
            .spacing(2)
            .push(widget::text::body(title))
            .push(widget::text::caption(subtitle).class(cosmic::style::Text::Color(theme_dim())))
            .into()
    }

    /// Wrap content with the blurred album-art backdrop + card, at a **fixed
    /// height**. The height MUST be definite: the backdrop fills via
    /// `Length::Fill`, and an unbounded Fill child collapses the tile's measured
    /// height in `flex_row` (taffy measures Fill as 0), so the next grid row
    /// would draw on top of the media tile (the "sysmon over media" bug).
    fn framed<'a>(
        &'a self,
        width: f32,
        height: f32,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        // Center the content within the fixed height; Fill so the backdrop (and
        // the card) cover the whole tile regardless of content size.
        let inner = widget::container(content)
            .width(Length::Fill)
            .center_y(Length::Fill);
        // The COSMIC style is a plain card; only the Framed style draws the
        // blurred album-art backdrop.
        let backdrop = (self.style == Style::Framed).then_some(()).and(self.art.as_ref());
        let stacked: Element<'a, Message> = match backdrop {
            Some((_, _, blurred)) => {
                let backdrop = widget::image(blurred.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Fill)
                    .opacity(0.40);
                Stack::new().push(inner).push_under(backdrop).into()
            }
            None => inner.into(),
        };
        widget::container(stacked)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .clip(true)
            .class(theme::card(self.state.playing, theme::accent()))
            .into()
    }
}

impl Module for MediaModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn min_cols(&self) -> u32 {
        2
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let cols = cols_for_width(width);

        // No active player.
        if self.conn.is_none() || (!self.state.has_player && self.state.title.is_empty()) {
            return super::tile(
                width,
                false,
                widget::Column::new()
                    .spacing(6)
                    .align_x(Alignment::Center)
                    .push(widget::icon::from_name(self.desc.icon.as_str()).size(22))
                    .push(widget::text::caption("Nothing playing")),
            );
        }

        // 2col (the minimum — media has min_cols 2): album art + play/pause over
        // the blurred backdrop. No text/progress/skip.
        if cols <= 2 {
            let row = widget::Row::new()
                .spacing(10)
                .align_y(Alignment::Center)
                .push(self.art_thumb(56.0))
                .push(widget::space::horizontal())
                .push(self.transport(id, edit, false));
            return self.framed(width, 80.0, widget::container(row).padding(12));
        }

        // 3–4col: rounded art + title/artist + full-width progress + transport.
        let mut right = widget::Column::new().spacing(8).push(self.info_text(cols));
        if self.state.length_us > 0 {
            let frac = self.scrubbing.unwrap_or(
                (self.state.position_us as f32 / self.state.length_us as f32).clamp(0.0, 1.0),
            );
            // Seekable → a real slider with a draggable handle; otherwise (or in
            // edit mode) a static progress bar.
            let bar: Element<'_, Message> = if edit || !self.state.can_seek {
                widget::progress_bar::determinate_linear(frac)
                    .width(Length::Fill)
                    .girth(Length::Fixed(6.0))
                    .into()
            } else {
                widget::slider(0.0..=1.0, frac, move |f| {
                    Message::Control(id, "seek".into(), ControlValue::Float(f as f64))
                })
                .on_release(Message::Control(id, "seek_commit".into(), ControlValue::Trigger))
                .step(0.001)
                .width(Length::Fill)
                .into()
            };
            right = right.push(bar);
        }
        right = right.push(self.transport(id, edit, true));

        let content = widget::container(
            widget::Row::new()
                .spacing(12)
                .align_y(Alignment::Center)
                .push(self.art_thumb(64.0))
                .push(right.width(Length::Fill)),
        )
        .padding(14);
        self.framed(width, 156.0, content)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "prev" => {
                if let Some(c) = &self.conn {
                    mpris::previous(c);
                }
            }
            "playpause" => {
                if let Some(c) = &self.conn {
                    mpris::play_pause(c);
                    self.state.playing = !self.state.playing; // optimistic
                }
            }
            "next" => {
                if let Some(c) = &self.conn {
                    mpris::next(c);
                }
            }
            "seek" => {
                if let ControlValue::Float(f) = value {
                    self.scrubbing = Some(f as f32);
                }
            }
            "seek_commit" => {
                if let Some(f) = self.scrubbing.take() {
                    if self.state.can_seek {
                        if let Some(tid) = self.state.track_id.clone() {
                            let pos = (f as f64 * self.state.length_us as f64) as i64;
                            if let Some(c) = self.conn.as_ref() {
                                mpris::set_position(c, &tid, pos);
                            }
                            self.state.position_us = pos;
                        }
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self, id: InstanceId) -> Task<Message> {
        let conn = self.conn.clone();
        let cur = self.art.as_ref().map(|(p, ..)| p.clone());
        super::fetch_task(id, move || fetch(conn, cur))
    }

    fn apply_data(&mut self, data: &dyn std::any::Any) {
        if let Some(d) = data.downcast_ref::<MediaData>() {
            self.set(d.clone());
        }
    }
}

fn theme_dim() -> cosmic::iced::Color {
    // Track light/dark so the artist caption stays legible after a theme switch.
    crate::theme::alpha(crate::theme::fg(), 0.6)
}
