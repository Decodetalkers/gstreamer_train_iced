use gst::glib;
use gst::prelude::*;
use gst::GenericFormattedValue;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::futures::SinkExt;
use iced::widget::image;
use iced::Command;
use smol::lock::Mutex as AsyncMutex;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use std::sync::mpsc;

pub mod reexport {
    pub use url;
}

static MEDIA_PLAYER: &[u8] = include_bytes!("../resource/popandpipi.jpg");

#[derive(Debug, Clone, Copy)]
pub enum PlayStatus {
    Stop,
    Playing,
    End,
}

#[derive(Debug)]
pub struct GstreamerIced {
    frame: Arc<Mutex<Option<image::Handle>>>, //pipeline: gst::Pipeline,
    bus: gst::Bus,
    source: gst::Bin,
    play_status: PlayStatus,
    rv: Arc<AsyncMutex<mpsc::Receiver<GStreamerMessage>>>,
    duration: std::time::Duration,
    position: std::time::Duration,
    info_get_started: bool,
    volume: f64,
    is_pipewire: bool,
}

#[derive(Debug, Error)]
pub enum Error {
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
}

impl Drop for GstreamerIced {
    fn drop(&mut self) {
        self.source
            .set_state(gst::State::Null)
            .expect("failed to set state");
    }
}

impl GstreamerIced {
    pub fn volume(&self) -> f64 {
        self.volume
    }

    pub fn set_volume(&mut self, volume: f64) {
        if self.is_pipewire {
            return;
        }
        self.source.set_property("volume", volume);
    }

    pub fn duration(&self) -> std::time::Duration {
        self.duration
    }

    pub fn position(&self) -> std::time::Duration {
        self.position
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration.as_secs_f64()
    }

    pub fn position_seconds(&self) -> f64 {
        self.position.as_secs_f64()
    }

    pub fn duration_nanos(&self) -> f64 {
        self.duration.as_secs_f64()
    }

    pub fn position_nanos(&self) -> u128 {
        self.position.as_nanos()
    }

    pub fn seek<T>(&mut self, position: T) -> Result<(), Error>
    where
        T: Into<Position>,
    {
        if self.is_pipewire {
            return Ok(());
        }
        let pos: Position = position.into();
        let positon: GenericFormattedValue = pos.into();
        self.source.seek_simple(gst::SeekFlags::FLUSH, positon)?;

        if let PlayStatus::End = self.play_status {
            self.play_status = PlayStatus::Playing;
        }

        Ok(())
    }

    pub fn frame_handle(&self) -> image::Handle {
        self.frame
            .lock()
            .map(|frame| {
                frame
                    .clone()
                    .unwrap_or(image::Handle::from_memory(MEDIA_PLAYER))
            })
            .unwrap_or(image::Handle::from_memory(MEDIA_PLAYER))
    }

    pub fn play_status(&self) -> &PlayStatus {
        &self.play_status
    }

    fn is_playing(&self) -> bool {
        matches!(self.play_status, PlayStatus::Playing)
    }

    /// Accept a pipewire stream
    pub fn new_pipewire(path: u32) -> Result<Self, Error> {
        gst::init()?;

        let source = gst::parse_launch(&format!("pipewiresrc path=\"{}\" ! videoconvert ! videoscale ! appsink name=app_sink caps=video/x-raw,format=RGBA,pixel-aspect-ratio=1/1 ", path))?;

        let source = source.downcast::<gst::Bin>().unwrap();

        let app_sink = source.by_name("app_sink").unwrap();
        let app_sink = app_sink.downcast::<gst_app::AppSink>().unwrap();
        let frame: Arc<Mutex<Option<image::Handle>>> = Arc::new(Mutex::new(None));
        let frame_ref = Arc::clone(&frame);

        let (sd, rv) = mpsc::channel::<GStreamerMessage>();
        app_sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    let pad = sink.static_pad("sink").ok_or(gst::FlowError::Error)?;

                    let caps = pad.current_caps().ok_or(gst::FlowError::Error)?;
                    let s = caps.structure(0).ok_or(gst::FlowError::Error)?;
                    let width = s.get::<i32>("width").map_err(|_| gst::FlowError::Error)?;
                    let height = s.get::<i32>("height").map_err(|_| gst::FlowError::Error)?;
                    *frame_ref.lock().map_err(|_| gst::FlowError::Error)? =
                        Some(image::Handle::from_pixels(
                            width as _,
                            height as _,
                            map.as_slice().to_owned(),
                        ));
                    sd.send(GStreamerMessage::FrameUpdate).ok();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        source.set_state(gst::State::Playing)?;

        Ok(Self {
            frame,
            bus: source.bus().unwrap(),
            source,
            play_status: PlayStatus::Playing,
            rv: Arc::new(AsyncMutex::new(rv)),
            duration: std::time::Duration::from_nanos(0),
            position: std::time::Duration::from_nanos(0),
            info_get_started: true,
            volume: 0_f64,
            is_pipewire: true,
        })
    }

    pub fn new_url(url: &url::Url, islive: bool) -> Result<Self, Error> {
        gst::init()?;
        let source = gst::parse_launch(&format!("playbin uri=\"{}\" video-sink=\"videoconvert ! videoscale ! appsink name=app_sink caps=video/x-raw,format=RGBA,pixel-aspect-ratio=1/1\"", url.as_str()))?;
        let source = source.downcast::<gst::Bin>().unwrap();

        let video_sink: gst::Element = source.property("video-sink");
        let pad = video_sink.pads().get(0).cloned().unwrap();
        let pad = pad.dynamic_cast::<gst::GhostPad>().unwrap();
        let bin = pad
            .parent_element()
            .unwrap()
            .downcast::<gst::Bin>()
            .unwrap();

        let app_sink = bin.by_name("app_sink").unwrap();
        let app_sink = app_sink.downcast::<gst_app::AppSink>().unwrap();
        let frame: Arc<Mutex<Option<image::Handle>>> = Arc::new(Mutex::new(None));
        let frame_ref = Arc::clone(&frame);

        let (sd, rv) = mpsc::channel::<GStreamerMessage>();
        app_sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    let pad = sink.static_pad("sink").ok_or(gst::FlowError::Error)?;

                    let caps = pad.current_caps().ok_or(gst::FlowError::Error)?;
                    let s = caps.structure(0).ok_or(gst::FlowError::Error)?;
                    let width = s.get::<i32>("width").map_err(|_| gst::FlowError::Error)?;
                    let height = s.get::<i32>("height").map_err(|_| gst::FlowError::Error)?;

                    *frame_ref.lock().map_err(|_| gst::FlowError::Error)? =
                        Some(image::Handle::from_pixels(
                            width as _,
                            height as _,
                            map.as_slice().to_owned(),
                        ));
                    sd.send(GStreamerMessage::FrameUpdate).ok();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        Ok(Self {
            frame,
            bus: source.bus().unwrap(),
            source,
            play_status: PlayStatus::Stop,
            rv: Arc::new(AsyncMutex::new(rv)),
            duration: std::time::Duration::from_nanos(0),
            position: std::time::Duration::from_nanos(0),
            info_get_started: !islive,
            volume: 0_f64,
            is_pipewire: false,
        })
    }

    pub fn subscription(&self) -> iced::Subscription<GStreamerMessage> {
        if self.is_playing() {
            let rv = self.rv.clone();
            iced::Subscription::batch([
                iced::time::every(std::time::Duration::from_secs_f64(0.01))
                    .map(|_| GStreamerMessage::Update),
                iced::subscription::channel(
                    std::any::TypeId::of::<()>(),
                    100,
                    |mut output| async move {
                        let rv = rv.lock().await;
                        loop {
                            let Ok(message) = rv.recv() else {
                                continue;
                            };
                            let _ = output.send(message).await;
                        }
                    },
                ),
            ])
        } else {
            iced::Subscription::none()
        }
    }

    pub fn update(&mut self, message: GStreamerMessage) -> iced::Command<GStreamerMessage> {
        match message {
            GStreamerMessage::Update => {
                // get the info in the first time of dispatch
                if !self.is_pipewire {
                    if self.info_get_started {
                        loop {
                            self.source
                                .state(gst::ClockTime::from_seconds(5))
                                .0
                                .unwrap();

                            if let Some(time) = self.source.query_duration::<gst::ClockTime>() {
                                self.duration = std::time::Duration::from_nanos(time.nseconds());
                                break;
                            }
                        }
                        self.info_get_started = false;
                    }
                    if self.duration.as_nanos() != 0 {
                        loop {
                            if let Some(time) = self.source.query_position::<gst::ClockTime>() {
                                self.position = std::time::Duration::from_nanos(time.nseconds());
                                break;
                            }
                            self.source
                                .state(gst::ClockTime::from_seconds(5))
                                .0
                                .unwrap();
                        }
                    }
                    self.volume = self.source.property("volume");
                }

                for msg in self.bus.iter() {
                    match msg.view() {
                        gst::MessageView::Error(err) => panic!("{:#?}", err),
                        gst::MessageView::Eos(_eos) => {
                            self.play_status = PlayStatus::End;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            GStreamerMessage::PlayStatusChanged(status) => {
                match status {
                    PlayStatus::Playing => {
                        self.source.set_state(gst::State::Playing).unwrap();
                    }
                    PlayStatus::Stop => {
                        self.source.set_state(gst::State::Paused).unwrap();
                    }
                    _ => {}
                }
                self.play_status = status;
            }
            _ => {}
        }
        Command::none()
    }
}
