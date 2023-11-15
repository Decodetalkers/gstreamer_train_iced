use gst::prelude::*;
use gst::GenericFormattedValue;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::Command;
use smol::lock::Mutex as AsyncMutex;
use std::sync::{Arc, Mutex};
use futures::channel::mpsc;

use super::{FrameData, GStreamerMessage, GstreamerIced, IcedGStreamerError, PlayStatus, Position};

pub type GstreamerIcedBase = GstreamerIced<0>;

impl GstreamerIcedBase {
    pub fn seek<T>(&mut self, position: T) -> Result<(), IcedGStreamerError>
    where
        T: Into<Position>,
    {
        let pos: Position = position.into();
        let positon: GenericFormattedValue = pos.into();
        self.source.seek_simple(gst::SeekFlags::FLUSH, positon)?;

        if let PlayStatus::End = self.play_status {
            self.play_status = PlayStatus::Playing;
        }

        Ok(())
    }

    /// accept url like from local or from http
    pub fn new_url(url: &url::Url, islive: bool) -> Result<Self, IcedGStreamerError> {
        gst::init()?;

        let video_sink = gst::Bin::new();
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
        let videoscale = gst::ElementFactory::make("videoscale").build()?;

        let app_sink_caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
            .build();

        let app_sink: gst_app::AppSink = gst_app::AppSink::builder()
            .name("app_sink")
            .caps(&app_sink_caps)
            .build();

        let frame: Arc<Mutex<Option<FrameData>>> = Arc::new(Mutex::new(None));
        let frame_ref = Arc::clone(&frame);

        let (mut sd, rv) = mpsc::channel::<GStreamerMessage>(100);

        app_sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                    let caps = sample.caps().ok_or(gst::FlowError::Error)?;
                    let s = caps.structure(0).ok_or(gst::FlowError::Error)?;
                    let width = s.get::<i32>("width").map_err(|_| gst::FlowError::Error)?;
                    let height = s.get::<i32>("height").map_err(|_| gst::FlowError::Error)?;
                    *frame_ref.lock().map_err(|_| gst::FlowError::Error)? = Some(FrameData {
                        width: width as _,
                        height: height as _,
                        pixels: map.as_slice().to_owned(),
                    });
                    sd.try_send(GStreamerMessage::FrameUpdate).ok();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let app_sink: gst::Element = app_sink.into();

        video_sink.add_many([&videoconvert, &videoscale, &app_sink])?;
        gst::Element::link_many([&videoconvert, &videoscale, &app_sink])?;

        let staticpad = videoconvert.static_pad("sink").unwrap();
        let sinkgost = gst::GhostPad::builder_with_target(&staticpad)?.build();
        sinkgost.set_active(true)?;
        video_sink.add_pad(&sinkgost)?;

        let videosource = gst::ElementFactory::make("playbin")
            .property("uri", url.as_str())
            .property("video-sink", video_sink.to_value())
            .build()?;

        let source = videosource.downcast::<gst::Bin>().unwrap();

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
        })
    }

    // update for gstreamer base
    pub fn update(&mut self, message: GStreamerMessage) -> iced::Command<GStreamerMessage> {
        match message {
            GStreamerMessage::Update => {
                // get the info in the first time of dispatch
                if self.info_get_started {
                    loop {
                        // FIXME: move it to stream listener
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
            GStreamerMessage::BusGoToEnd => {
                self.play_status = PlayStatus::End;
            }
            _ => {}
        }
        Command::none()
    }

    /// get the volume of the video
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// only can be set when source is video
    pub fn set_volume(&mut self, volume: f64) {
        self.source.set_property("volume", volume);
    }

    /// get the duration, if is live or pipewire, it is 0
    pub fn duration(&self) -> std::time::Duration {
        self.duration
    }

    /// where the video is now
    pub fn position(&self) -> std::time::Duration {
        self.position
    }

    /// turn duration to seconds
    pub fn duration_seconds(&self) -> f64 {
        self.duration.as_secs_f64()
    }

    /// turn position to seconds
    pub fn position_seconds(&self) -> f64 {
        self.position.as_secs_f64()
    }

    /// turn duration to nanos
    pub fn duration_nanos(&self) -> f64 {
        self.duration.as_secs_f64()
    }

    /// turn position to nanos
    pub fn position_nanos(&self) -> u128 {
        self.position.as_nanos()
    }
}
