use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::Command;
use smol::lock::Mutex as AsyncMutex;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::{FrameData, GStreamerMessage, GstreamerIced, IcedGStreamerError, PlayStatus};

pub type GstreamerIcedPipewire = GstreamerIced<1>;

impl GstreamerIcedPipewire {
    /// Accept a pipewire stream, it accept a pipewire path, you may can get it from ashpd, it is
    /// called node.
    pub fn new_pipewire(path: u32) -> Result<Self, IcedGStreamerError> {
        gst::init()?;

        let source = gst::Pipeline::new();
        let pipewiresrc = gst::ElementFactory::make("pipewiresrc")
            .property("path", path.to_string())
            .build()?;

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

        let (sd, rv) = mpsc::channel::<GStreamerMessage>();

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
                    sd.send(GStreamerMessage::FrameUpdate).ok();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let app_sink: gst::Element = app_sink.into();
        source.add_many([&pipewiresrc, &videoconvert, &videoscale, &app_sink])?;

        gst::Element::link_many([&pipewiresrc, &videoconvert, &videoscale, &app_sink])?;

        source.set_state(gst::State::Playing)?;

        Ok(Self {
            frame,
            bus: source.bus().unwrap(),
            source: source.into(),
            play_status: PlayStatus::Playing,
            rv: Arc::new(AsyncMutex::new(rv)),
            duration: std::time::Duration::from_nanos(0),
            position: std::time::Duration::from_nanos(0),
            info_get_started: true,
            volume: 0_f64,
        })
    }

    /// update for pipewire
    pub fn update(&mut self, message: GStreamerMessage) -> iced::Command<GStreamerMessage> {
        match message {
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
}
