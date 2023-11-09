use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use iced::widget::{button, column, image, text, Image};
use iced::{executor, widget::container, Application, Theme};
use iced::{Command, Element, Length, Settings};
use num_traits::ToPrimitive;
use std::sync::{Arc, Mutex};

static MEDIA_PLAYER: &[u8] = include_bytes!("../resource/media-playback-start.svg");

#[derive(Debug, Default)]
struct InitFlage {
    url: String,
}

#[derive(Debug, Clone, Copy)]
enum PlayStatus {
    Stop,
    Start,
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
    frame: Arc<Mutex<Option<image::Handle>>>, //pipeline: gst::Pipeline,
    bus: gst::Bus,
    source: gst::Bin,
    play_status: PlayStatus,
    framerate: f64,
}

#[derive(Debug, Clone, Copy)]
struct GstreamerUpdate;

#[derive(Debug, Clone, Copy)]
enum GStreamerMessage {
    Update,
    PlayStatusChanged(PlayStatus),
}

impl Application for GstreamserIced {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GStreamerMessage;

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

        let btn: Element<Self::Message> =
            match self.play_status {
                PlayStatus::Stop => button(text("|>"))
                    .on_press(GStreamerMessage::PlayStatusChanged(PlayStatus::Start)),
                PlayStatus::Start => button(text("[]"))
                    .on_press(GStreamerMessage::PlayStatusChanged(PlayStatus::Stop)),
            }
            .into();
        let video = Image::new(frame).width(Length::Fill);

        container(column![
            video,
            container(btn).width(Length::Fill).center_x()
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
    }

    fn update(&mut self, message: Self::Message) -> iced::Command<Self::Message> {
        match message {
            GStreamerMessage::Update => {
                for msg in self.bus.iter() {
                    match msg.view() {
                        gst::MessageView::Error(err) => panic!("{:#?}", err),
                        gst::MessageView::Eos(_eos) => {
                            self.play_status = PlayStatus::Stop;
                            self.source.set_state(gst::State::Null).unwrap();
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
        }
        Command::none()
    }

    fn title(&self) -> String {
        "Iced ffmpeg".to_string()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::time::every(std::time::Duration::from_secs_f64(0.5 / self.framerate))
            .map(|_| GStreamerMessage::Update)
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
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        source.set_state(gst::State::Playing).unwrap();

        // wait for up to 5 seconds until the decoder gets the source capabilities
        source.state(gst::ClockTime::from_seconds(5)).0.unwrap();
        let caps = pad.current_caps().unwrap();
        let s = caps.structure(0).unwrap();
        let framerate = s.get::<gst::Fraction>("framerate").unwrap();

        // after get the information, paused it
        source.set_state(gst::State::Paused).unwrap();
        (
            Self {
                frame,
                bus: source.bus().unwrap(),
                source,
                play_status: PlayStatus::Stop,
                framerate: num_rational::Rational32::new(
                    framerate.numer() as _,
                    framerate.denom() as _,
                )
                .to_f64()
                .unwrap(),
            },
            Command::none(),
        )
    }
}
