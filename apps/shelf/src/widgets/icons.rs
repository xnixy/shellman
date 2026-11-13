//! The widgets' small pictures, drawn from the design's own SVG paths (weather, and the media
//! buttons) in white and tinted when painted, so one texture serves every colour. Each is drawn
//! once per pixel size and kept.

use egui::{Color32, Rect, TextureHandle, TextureOptions};
use shellman_core::weather::Sky;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pic {
    Sky(SkyPic),
    Play,
    Pause,
    Next,
    Previous,
}

/// `Sky`, hashable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SkyPic {
    Sun,
    Moon,
    SunCloud,
    MoonCloud,
    Cloud,
    Fog,
    Drizzle,
    Rain,
    Sleet,
    Snow,
    Thunder,
}

impl From<Sky> for SkyPic {
    fn from(s: Sky) -> Self {
        match s {
            Sky::Clear => SkyPic::Sun,
            Sky::ClearNight => SkyPic::Moon,
            Sky::PartlyCloudy => SkyPic::SunCloud,
            Sky::PartlyCloudyNight => SkyPic::MoonCloud,
            Sky::Cloudy => SkyPic::Cloud,
            Sky::Fog => SkyPic::Fog,
            Sky::Drizzle => SkyPic::Drizzle,
            Sky::Rain => SkyPic::Rain,
            Sky::Sleet => SkyPic::Sleet,
            Sky::Snow => SkyPic::Snow,
            Sky::Thunder => SkyPic::Thunder,
        }
    }
}

const CLOUD: &str = "M7 18a4.5 4.5 0 0 1 .3-9 5.5 5.5 0 0 1 10.5 1.8A3.6 3.6 0 0 1 17.5 18z";
const RAIN_CLOUD: &str = "M7 15.5a4 4 0 0 1 .3-8 5.2 5.2 0 0 1 9.9 1.6A3.3 3.3 0 0 1 17 15.5z";

/// The SVG body (inside a 24 x 24 view box) of a picture.
fn body(p: Pic) -> String {
    let stroke = |d: &str| format!(r#"<g fill="none" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{d}</g>"#);
    let path = |d: &str| format!(r#"<path d="{d}"/>"#);
    match p {
        Pic::Sky(s) => stroke(&match s {
            SkyPic::Sun => r#"<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>"#.to_owned(),
            SkyPic::Moon => path("M19.5 14.2A7.8 7.8 0 1 1 9.8 4.5a6.2 6.2 0 0 0 9.7 9.7z"),
            SkyPic::SunCloud => format!(
                r#"<path d="M9.2 4.6v-1.4M3.6 10.2H2.2M5.2 6.2l-1-1M13.2 6.2l1-1"/><path d="M5.6 12.4a3.6 3.6 0 0 1 6.6-3.6"/><path d="M8.5 20a3.8 3.8 0 0 1 .3-7.6 4.7 4.7 0 0 1 9 1.5A3.1 3.1 0 0 1 17.5 20z"/>"#
            ),
            SkyPic::MoonCloud => format!(
                r#"<path d="M12.6 8.4A4.9 4.9 0 0 1 6.4 2.8a5.6 5.6 0 0 0-1.2 9.6"/><path d="M8.5 20a3.8 3.8 0 0 1 .3-7.6 4.7 4.7 0 0 1 9 1.5A3.1 3.1 0 0 1 17.5 20z"/>"#
            ),
            SkyPic::Cloud => path(CLOUD),
            SkyPic::Fog => format!(r#"{}<path d="M4 20.5h8M15 20.5h5"/>"#, path("M7 15a4 4 0 0 1 .3-8 5.2 5.2 0 0 1 9.9 1.6A3.3 3.3 0 0 1 17 15z")),
            SkyPic::Drizzle => format!(r#"{}<path d="M9 19v.6M13 19v.6M17 19v.6"/>"#, path(RAIN_CLOUD)),
            SkyPic::Rain => format!(r#"{}<path d="M8.5 18.5l-.8 1.8M12.5 18.5l-.8 1.8M16.5 18.5l-.8 1.8"/>"#, path(RAIN_CLOUD)),
            SkyPic::Sleet => format!(r#"{}<path d="M8.5 18.5l-.8 1.8M16.5 18.5l-.8 1.8"/><circle cx="12.2" cy="19.6" r=".6"/>"#, path(RAIN_CLOUD)),
            SkyPic::Snow => format!(r#"{}<path d="M8.5 18.3v2.4M7.3 19.5h2.4M15.5 18.3v2.4M14.3 19.5h2.4"/>"#, path(RAIN_CLOUD)),
            SkyPic::Thunder => format!(r#"{}<path d="M12.5 15.5l-2 3.2h3l-2 3.3"/>"#, path(RAIN_CLOUD)),
        }),
        Pic::Play => r#"<path fill="white" d="M8 5.5v13l10.5-6.5z"/>"#.to_owned(),
        Pic::Pause => r#"<g fill="white"><rect x="6.5" y="5" width="4" height="14" rx="1.3"/><rect x="13.5" y="5" width="4" height="14" rx="1.3"/></g>"#.to_owned(),
        Pic::Next => r#"<g fill="white"><path d="M5 5.5v13l10-6.5z"/><rect x="16" y="5.5" width="3" height="13" rx="1"/></g>"#.to_owned(),
        Pic::Previous => r#"<g fill="white" transform="translate(24 0) scale(-1 1)"><path d="M5 5.5v13l10-6.5z"/><rect x="16" y="5.5" width="3" height="13" rx="1"/></g>"#.to_owned(),
    }
}

#[derive(Default)]
pub struct Icons {
    cache: HashMap<(Pic, u32), Option<TextureHandle>>,
}

impl Icons {
    /// Paint `p` filling `rect` (points) in `color`.
    pub fn paint(&mut self, ui: &egui::Ui, p: Pic, rect: Rect, color: Color32) {
        let px = (rect.width().max(rect.height()) * ui.ctx().pixels_per_point()).ceil().clamp(8.0, 256.0) as u32;
        let tex = self.cache.entry((p, px)).or_insert_with(|| {
            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">{}</svg>"#, body(p));
            let image = shellman_core::icon_pack::raster::svg_data(svg.as_bytes(), px)?;
            Some(ui.ctx().load_texture(format!("widget-icon-{p:?}-{px}"), image, TextureOptions::LINEAR))
        });
        if let Some(t) = tex {
            ui.painter().image(t.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_draws() {
        let all = [
            SkyPic::Sun, SkyPic::Moon, SkyPic::SunCloud, SkyPic::MoonCloud, SkyPic::Cloud, SkyPic::Fog,
            SkyPic::Drizzle, SkyPic::Rain, SkyPic::Sleet, SkyPic::Snow, SkyPic::Thunder,
        ];
        let pics = all.into_iter().map(Pic::Sky).chain([Pic::Play, Pic::Pause, Pic::Next, Pic::Previous]);
        for p in pics {
            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">{}</svg>"#, body(p));
            let img = shellman_core::icon_pack::raster::svg_data(svg.as_bytes(), 48).unwrap_or_else(|| panic!("{p:?}"));
            assert!(img.pixels.iter().any(|c| c.a() > 0), "{p:?} is empty");
        }
    }
}
