use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::futures::SinkExt;
use iced::widget::{image, Image};
use iced::{executor, subscription, widget::container, Application, Theme};
use iced::{Command, Length, Settings};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex as TokioMutex;

static MEDIA_PLAYER: &[u8] = include_bytes!("../resource/media-playback-start.svg");

#[derive(Debug, Default)]
struct InitFlage {
    url: String,
}

fn main() -> iced::Result {
    GstreamserIced::run(Settings {
        flags: InitFlage {
            url:
                "http://commondatastorage.googleapis.com/gtv-videos-bucket/sample/TearsOfSteel.mp4"
                    .to_string(),
        },
        ..Settings::default()
    })
}

#[derive(Debug)]
struct GstreamserIced {
    rv: Arc<TokioMutex<Receiver<GstreamerMessage>>>,
    frame: Arc<Mutex<Option<image::Handle>>>, //pipeline: gst::Pipeline,
    bus: gst::Bus,
    source: gst::Bin,
}

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
enum GstreamerMessage {
    Update,
    Todo,
}

impl Application for GstreamserIced {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GstreamerMessage;

    fn view(&self) -> iced::Element<Self::Message> {
        let frame = self
            .frame
            .lock()
            .map(|frame| {
                frame
                    .clone()
                    .unwrap_or(image::Handle::from_memory(MEDIA_PLAYER))
            })
            .unwrap_or(image::Handle::from_memory(MEDIA_PLAYER));

        let video = Image::new(frame).width(Length::Fill);
        container(video)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }

    fn update(&mut self, message: Self::Message) -> iced::Command<Self::Message> {
        if let GstreamerMessage::Update = message {
            for msg in self.bus.iter() {
                match msg.view() {
                    gst::MessageView::Error(err) => panic!("{:#?}", err),
                    gst::MessageView::Eos(_eos) => {
                        self.source.set_state(gst::State::Null).unwrap();
                        break;
                    }
                    _ => {}
                }
            }
        }
        Command::none()
    }

    fn title(&self) -> String {
        "Iced ffmpeg".to_string()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        let rv = self.rv.clone();
        subscription::channel(std::any::TypeId::of::<()>(), 100, |mut output| async move {
            let mut rv = rv.lock().await;
            loop {
                let Some(message) = rv.recv().await else {
                    continue;
                };
                let _ = output.send(message).await;
            }
        })
    }

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        gst::init().unwrap();
        let source = gst::parse_launch(&format!("playbin uri=\"{}\" video-sink=\"videoconvert ! videoscale ! appsink name=app_sink caps=video/x-raw,format=BGRA,pixel-aspect-ratio=1/1\"", flags.url)).unwrap();
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
        let (sd, rv) = tokio::sync::mpsc::channel::<GstreamerMessage>(100);

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
                    sd.try_send(GstreamerMessage::Update).ok();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        source.set_state(gst::State::Playing).unwrap();

        (
            Self {
                frame,
                rv: Arc::new(TokioMutex::new(rv)),
                bus: source.bus().unwrap(),
                source,
            },
            Command::none(),
        )
    }
}
