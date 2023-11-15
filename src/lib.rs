mod gstreamerbase;
mod gstreamerpipewire;

use futures::channel::mpsc;
use gst::glib;
use gst::prelude::*;
use gst::GenericFormattedValue;
use gstreamer as gst;
use iced::futures::SinkExt;
use iced::futures::StreamExt;
use iced::widget::image;
use smol::lock::Mutex as AsyncMutex;
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub mod reexport {
    pub use url;
}

#[derive(Debug, Clone, Copy)]
pub enum PlayStatus {
    Stop,
    Playing,
    End,
}

#[derive(Debug, Clone)]
pub struct FrameData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl From<FrameData> for image::Handle {
    fn from(
        FrameData {
            pixels,
            width,
            height,
        }: FrameData,
    ) -> Self {
        image::Handle::from_pixels(width, height, pixels)
    }
}

pub use gstreamerbase::GstreamerIcedBase;

pub use gstreamerpipewire::GstreamerIcedPipewire;

#[derive(Debug)]
pub struct GstreamerIced<const X: usize> {
    frame: Arc<Mutex<Option<FrameData>>>, //pipeline: gst::Pipeline,
    bus: gst::Bus,
    source: gst::Bin,
    play_status: PlayStatus,
    rv: Arc<AsyncMutex<mpsc::Receiver<GStreamerMessage>>>,
    duration: std::time::Duration,
    position: std::time::Duration,
    info_get_started: bool,
    volume: f64,
}

#[derive(Debug, Error)]
pub enum IcedGStreamerError {
    #[error("{0}")]
    Glib(#[from] glib::Error),
    #[error("{0}")]
    Bool(#[from] glib::BoolError),
    #[error("failed to get the gstreamer bus")]
    Bus,
    #[error("{0}")]
    StateChange(#[from] gst::StateChangeError),
    #[error("failed to cast gstreamer element")]
    Cast,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid URI")]
    Uri,
    #[error("failed to get media capabilities")]
    Caps,
    #[error("failed to query media duration or position")]
    Duration,
    #[error("failed to sync with playback")]
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Position {
    /// Position based on time.
    ///
    /// Not the most accurate format for videos.
    Time(std::time::Duration),
    /// Position based on nth frame.
    Frame(u64),
}

impl From<Position> for GenericFormattedValue {
    fn from(pos: Position) -> Self {
        match pos {
            Position::Time(t) => gst::ClockTime::from_nseconds(t.as_nanos() as _).into(),
            Position::Frame(f) => gst::format::Default::from_u64(f).into(),
        }
    }
}

impl From<std::time::Duration> for Position {
    fn from(t: std::time::Duration) -> Self {
        Position::Time(t)
    }
}

impl From<u64> for Position {
    fn from(f: u64) -> Self {
        Position::Frame(f)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GStreamerMessage {
    Update,
    FrameUpdate,
    PlayStatusChanged(PlayStatus),
    BusGoToEnd,
}

impl<const X: usize> Drop for GstreamerIced<X> {
    fn drop(&mut self) {
        self.source
            .set_state(gst::State::Null)
            .expect("failed to set state");
    }
}

impl<const X: usize> GstreamerIced<X> {
    /// return an [image::Handle], you can use it to make image
    pub fn frame_handle(&self) -> Option<image::Handle> {
        self.frame
            .lock()
            .map(|frame| frame.clone().map(|f| f.into()))
            .unwrap_or(None)
    }

    /// return [FrameData], you can directly access the data
    pub fn frame_data(&self) -> Option<FrameData> {
        self.frame.lock().map(|frame| frame.clone()).unwrap_or(None)
    }

    /// what the playing status is
    pub fn play_status(&self) -> &PlayStatus {
        &self.play_status
    }

    fn is_playing(&self) -> bool {
        matches!(self.play_status, PlayStatus::Playing)
    }

    /// get the subscription, you can use in iced::subscription
    pub fn subscription(&self) -> iced::Subscription<GStreamerMessage> {
        if self.is_playing() {
            let rv = self.rv.clone();
            let bus = self.bus.clone();
            struct BusWatcher;
            iced::Subscription::batch([
                iced::time::every(std::time::Duration::from_secs_f64(0.05))
                    .map(|_| GStreamerMessage::Update),
                iced::subscription::channel(
                    std::any::TypeId::of::<()>(),
                    100,
                    |mut output| async move {
                        let mut rv = rv.lock().await;
                        loop {
                            let Some(message) = rv.next().await else {
                                continue;
                            };
                            let _ = output.send(message).await;
                        }
                    },
                ),
                iced::subscription::channel(
                    std::any::TypeId::of::<BusWatcher>(),
                    100,
                    |mut output| async move {
                        let mut thebus = bus.stream();
                        while let Some(view) = thebus.next().await {
                            match view.view() {
                                gst::MessageView::Error(err) => panic!("{:#?}", err),
                                gst::MessageView::Eos(_eos) => {
                                    let _ = output.send(GStreamerMessage::BusGoToEnd).await;
                                }
                                _ => {}
                            }
                        }
                        loop {
                            // DO NOTHING here
                            futures_time::task::sleep(futures_time::time::Duration::from_millis(
                                10,
                            ))
                            .await;
                        }
                    },
                ),
            ])
        } else {
            iced::Subscription::none()
        }
    }

    /// get the type name
    pub fn gstreamer_type(&self) -> String {
        match X {
            0 => "base".to_owned(),
            1 => "pipewire".to_owned(),
            _ => unreachable!(),
        }
    }
}
