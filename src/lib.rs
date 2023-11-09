use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::futures::SinkExt;
use iced::widget::image;
use iced::Command;
use smol::lock::Mutex as AsyncMutex;
use std::sync::{Arc, Mutex};

use std::sync::mpsc;

static MEDIA_PLAYER: &[u8] = include_bytes!("../resource/media-playback-start.svg");

#[derive(Debug, Clone, Copy)]
pub enum PlayStatus {
    Stop,
    Start,
}

#[derive(Debug)]
pub struct GstreamserIced {
    frame: Arc<Mutex<Option<image::Handle>>>, //pipeline: gst::Pipeline,
    bus: gst::Bus,
    source: gst::Bin,
    play_status: PlayStatus,
    rv: Arc<AsyncMutex<mpsc::Receiver<GStreamerMessage>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum GStreamerMessage {
    Update,
    FrameUpdate,
    PlayStatusChanged(PlayStatus),
}

impl Drop for GstreamserIced {
    fn drop(&mut self) {
        self.source
            .set_state(gst::State::Null)
            .expect("failed to set state");
    }
}

impl GstreamserIced {
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
        matches!(self.play_status, PlayStatus::Start)
    }

    pub fn new_url(url: &str, _islive: bool) -> Self {
        gst::init().unwrap();
        let source = gst::parse_launch(&format!("playbin uri=\"{}\" video-sink=\"videoconvert ! videoscale ! appsink name=app_sink caps=video/x-raw,format=BGRA,pixel-aspect-ratio=1/1\"", url)).unwrap();
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

        Self {
            frame,
            bus: source.bus().unwrap(),
            source,
            play_status: PlayStatus::Stop,
            rv: Arc::new(AsyncMutex::new(rv)),
        }
    }

    pub fn subscription(&self) -> iced::Subscription<GStreamerMessage> {
        if self.is_playing() {
            let rv = self.rv.clone();
            iced::Subscription::batch([
                iced::time::every(std::time::Duration::from_secs_f64(0.05))
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
                for msg in self.bus.iter() {
                    match msg.view() {
                        gst::MessageView::Error(err) => panic!("{:#?}", err),
                        gst::MessageView::Eos(_eos) => {
                            self.play_status = PlayStatus::Stop;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            GStreamerMessage::PlayStatusChanged(status) => {
                match status {
                    PlayStatus::Start => {
                        self.source.set_state(gst::State::Playing).unwrap();
                    }
                    PlayStatus::Stop => {
                        self.source.set_state(gst::State::Paused).unwrap();
                    }
                }
                self.play_status = status;
            }
            _ => {}
        }
        Command::none()
    }
}
