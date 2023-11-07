use iced::widget::{image, text, Image};
use iced::{executor, widget::container, Application, Theme};
use iced::{Command, Length, Settings};

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;

#[derive(Debug, Default)]
struct InitFlage {
    url: String,
}

fn main() -> iced::Result {
    GstreamserIced::run(Settings {
        flags: InitFlage {
            url: "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm"
                .to_string(),
        },
        ..Settings::default()
    })
}

#[derive(Debug)]
struct GstreamserIced {
    url: String,
    //pipeline: gst::Pipeline,
}

#[derive(Debug, Clone, Copy)]
enum GstreamerMessage {}

impl Application for GstreamserIced {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GstreamerMessage;
    fn view(&self) -> iced::Element<Self::Message> {
        container(text("test"))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
    fn update(&mut self, _message: Self::Message) -> iced::Command<Self::Message> {
        Command::none()
    }
    fn title(&self) -> String {
        "Test".to_string()
    }
    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        gst::init().unwrap();

        let videoconvert = gst::ElementFactory::make("videoconvert").build().unwrap();
        let videoscale = gst::ElementFactory::make("videoscale").build().unwrap();
        let cap = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("pixel-aspect-ratio", "1/1")
            .build();

        let appsink = gst::ElementFactory::make("appsink")
            .property("name", "app_sink")
            .property("caps", cap.to_value())
            .build()
            .unwrap();

        let video_sink_pipeline = gst::Pipeline::new();
        video_sink_pipeline
            .add_many(&[&videoconvert, &videoscale, &appsink])
            .unwrap();

        let source = gst::ElementFactory::make("playbin")
            .property("uri", flags.url.as_str())
            .property("video-sink", video_sink_pipeline.to_value())
            .build()
            .unwrap();

        let app_sink = appsink.downcast::<gst_app::AppSink>().unwrap();
        app_sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    println!("sss");
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        source.set_state(gst::State::Playing).unwrap();
        source.state(gst::ClockTime::from_seconds(5)).0.unwrap();

        let bus = source.bus().unwrap();
        'out: loop {
            for msg in bus.iter() {
                if let gst::MessageView::Eos(eos) = msg.view() {
                    break 'out;
                }
            }
        }

        (Self { url: flags.url }, Command::none())
    }
}
