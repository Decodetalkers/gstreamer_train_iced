use anyhow::anyhow;
use ashpd::{
    desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType},
    WindowIdentifier,
};

use iced::widget::{button, column, image, text, Image};
use iced::{executor, widget::container, Application, Theme};
use iced::{Command, Element, Length, Settings};

static MEDIA_PLAYER: &[u8] = include_bytes!("../resource/popandpipi.jpg");
use gstreamer_iced::*;

async fn get_path() -> anyhow::Result<u32> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    proxy
        .select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Monitor | SourceType::Window,
            true,
            None,
            PersistMode::DoNot,
        )
        .await?;

    let response = proxy
        .start(&session, &WindowIdentifier::default())
        .await?
        .response()?;
    for stream in response.streams().iter() {
        println!("node id: {}", stream.pipe_wire_node_id());
        println!("size: {:?}", stream.size());
        println!("position: {:?}", stream.position());
        return Ok(stream.pipe_wire_node_id());
    }
    Err(anyhow!("Not get"))
}

#[tokio::main]
async fn main() -> iced::Result {
    let path = get_path().await.unwrap();
    GstreamerIcedProgram::run(Settings {
        flags: InitFlage { path },
        ..Settings::default()
    })
}

#[derive(Debug, Default)]
struct InitFlage {
    path: u32,
}

struct GstreamerIcedProgram {
    frame: GstreamerIcedPipewire,
}
#[derive(Debug, Clone, Copy)]
enum GStreamerIcedMessage {
    Gst(GStreamerMessage),
}

#[derive(Debug, Clone, Copy)]
struct GstreamerUpdate;

impl Application for GstreamerIcedProgram {
    type Theme = Theme;
    type Flags = InitFlage;
    type Executor = executor::Default;
    type Message = GStreamerIcedMessage;

    fn view(&self) -> iced::Element<Self::Message> {
        let frame = self
            .frame
            .frame_handle()
            .unwrap_or(image::Handle::from_memory(MEDIA_PLAYER));

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
        let GStreamerIcedMessage::Gst(message) = message;
        self.frame.update(message).map(GStreamerIcedMessage::Gst)
    }

    fn title(&self) -> String {
        "Iced Gstreamer".to_string()
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        self.frame.subscription().map(GStreamerIcedMessage::Gst)
    }

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let frame =
            GstreamerIced::new_pipewire_with_record(flags.path, "/tmp/aa.mp4").unwrap();

        (Self { frame }, Command::none())
    }
}
