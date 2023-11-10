use iced::widget::{button, column, row, slider, text, Image};
use iced::{executor, widget::container, Application, Theme};
use iced::{Command, Element, Length, Settings};

use gstreamer_iced::*;

#[derive(Debug, Default)]
struct InitFlage {
    url: Option<url::Url>,
}

fn main() -> iced::Result {
    GstreamerIcedProgram::run(Settings {
        flags: InitFlage {
            url: Some(
                url::Url::parse(
                    "http://commondatastorage.googleapis.com/gtv-videos-bucket/sample/TearsOfSteel.mp4",
                    //"https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm",
                )
                .unwrap(),
            ),
        },
        ..Settings::default()
    })
}

#[derive(Debug)]
struct GstreamerIcedProgram {
    frame: GstreamerIced,
}
#[derive(Debug, Clone, Copy)]
enum GStreamerIcedMessage {
    Gst(GStreamerMessage),
    Jump(u8),
}

#[derive(Debug, Clone, Copy)]
struct GstreamerUpdate;

impl Application for GstreamerIcedProgram {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GStreamerIcedMessage;

    fn view(&self) -> iced::Element<Self::Message> {
        let frame = self.frame.frame_handle();
        let fullduration = self.frame.duration_seconds();
        let current_pos = self.frame.position_seconds();
        let duration = (fullduration / 8.0) as u8;
        let pos = (current_pos / 8.0) as u8;

        let btn: Element<Self::Message> = match self.frame.play_status() {
            PlayStatus::Stop | PlayStatus::End => button(text("|>")).on_press(
                GStreamerIcedMessage::Gst(GStreamerMessage::PlayStatusChanged(PlayStatus::Playing)),
            ),
            PlayStatus::Playing => button(text("[]")).on_press(GStreamerIcedMessage::Gst(
                GStreamerMessage::PlayStatusChanged(PlayStatus::Stop),
            )),
        }
        .into();
        let video = Image::new(frame).width(Length::Fill);

        let pos_status = text(format!("{:.1} s/{:.1} s", current_pos, fullduration));
        let du_silder: Element<Self::Message> =
            slider(0..=duration, pos, GStreamerIcedMessage::Jump).into();
        let duration_component = row![pos_status, du_silder]
            .spacing(2)
            .padding(2)
            .width(Length::Fill);

        container(column![
            video,
            duration_component,
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
            GStreamerIcedMessage::Gst(message) => {
                self.frame.update(message).map(GStreamerIcedMessage::Gst)
            }
            GStreamerIcedMessage::Jump(step) => {
                self.frame
                    .seek(std::time::Duration::from_secs(step as u64 * 8))
                    .unwrap();
                Command::perform(
                    async { GStreamerMessage::Update },
                    GStreamerIcedMessage::Gst,
                )
            }
        }
    }

    fn title(&self) -> String {
        "Iced Gstreamer".to_string()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        self.frame.subscription().map(GStreamerIcedMessage::Gst)
    }

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let frame = GstreamerIced::new_url(flags.url.as_ref().unwrap(), false).unwrap();

        (Self { frame }, Command::none())
    }
}
