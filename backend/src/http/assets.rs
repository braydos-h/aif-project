//! Compile-time static assets and the controlled demo-image registry.
//!
//! Static routes are explicit: the WebUI files are embedded with
//! `include_bytes!` and demo images come from a fixed list, so no request
//! can read an arbitrary filesystem path.

pub(crate) const INDEX_HTML: &[u8] = include_bytes!("../../../web/index.html");
pub(crate) const STYLES_CSS: &[u8] = include_bytes!("../../../web/styles.css");
pub(crate) const APP_JS: &[u8] = include_bytes!("../../../web/app.js");

/// The only demo files exposed by the server. They are compiled into the
/// binary so the route cannot escape the approved `cows/` directory.
#[derive(Clone, Copy)]
pub(crate) struct DemoCow {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) mime_type: &'static str,
    pub(crate) body: &'static [u8],
}

pub(crate) const DEMO_COWS: &[DemoCow] = &[
    DemoCow {
        id: "1",
        name: "cow 1.webp",
        mime_type: "image/webp",
        body: include_bytes!("../../../cows/cow 1.webp"),
    },
    DemoCow {
        id: "2",
        name: "cow 2.jpg",
        mime_type: "image/jpeg",
        body: include_bytes!("../../../cows/cow 2.jpg"),
    },
    DemoCow {
        id: "3",
        name: "cow 3.jpg",
        mime_type: "image/jpeg",
        body: include_bytes!("../../../cows/cow 3.jpg"),
    },
];
