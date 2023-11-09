use iced::widget::{button, column, text, Image};
use iced::{executor, widget::container, Application, Theme};
use iced::{Command, Element, Length, Settings};

use gstreamer_iced::*;

#[derive(Debug, Default)]
struct InitFlage {
    url: Option<url::Url>,
}

fn main() -> iced::Result {
    GstreamserIcedProgram::run(Settings {
        flags: InitFlage {
            url: Some(
                url::Url::parse(
                    "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm",
                )
                .unwrap(),
            ),
        },
        ..Settings::default()
    })
}

#[derive(Debug)]
struct GstreamserIcedProgram {
    frame: GstreamserIced,
}

#[derive(Debug, Clone, Copy)]
struct GstreamerUpdate;

impl Application for GstreamserIcedProgram {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GStreamerMessage;

    fn view(&self) -> iced::Element<Self::Message> {
        let frame = self.frame.frame_handle();

        let btn: Element<Self::Message> =
            match self.frame.play_status() {
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
        self.frame.update(message)
    }

    fn title(&self) -> String {
        "Iced ffmpeg".to_string()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        self.frame.subscription()
    }

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let frame = GstreamserIced::new_url(flags.url.as_ref().unwrap(), true);

        (Self { frame }, Command::none())
    }
}
