//! The display service's screen-sharing session: an audio stream first (it
//! sets up the session the phone's negotiator keys video on), then video on
//! the same client session id, RTP/HEVC to a UDP port on the tunnel.

use idevice::core_device::{
    CallInfoBlob, DisplayServiceClient, build_screen_audio_offer, build_screen_video_offer,
    build_start_audio_parameters, build_start_video_parameters,
};
use idevice::rsd::RsdHandshake;
use idevice::tcp::handle::{AdapterHandle, UdpSocketHandle};
use idevice::{ReadWrite, RsdService};

/// What the controller supports, not the phone's mask: Xcode sends 140, and
/// the phone's larger mask makes its negotiator produce an invalid config.
const CLIENT_SUPPORTED_FEATURES: u64 = 140;

pub struct Screen {
    pub client: DisplayServiceClient<Box<dyn ReadWrite>>,
    pub audio: UdpSocketHandle,
    pub video: UdpSocketHandle,
    pub audio_rtcp: Rtcp,
    pub video_rtcp: Rtcp,
}

/// Where a stream's RTCP goes and the two SSRCs it names, out of the
/// answer's `streamConfig` (written from the phone's side: its `LocalSSRC`
/// is the sender's, `RemoteSSRC` ours).
#[derive(Clone, Copy, Debug, Default)]
pub struct Rtcp {
    pub port: u16,
    pub ours: u32,
    pub phone: u32,
}

fn rtcp(answer: &plist::Value, fallback_ours: u32) -> Rtcp {
    let cfg = answer
        .as_dictionary()
        .and_then(|d| d.get("connection"))
        .and_then(|c| c.as_dictionary())
        .and_then(|c| c.get("streamConfig"))
        .and_then(|c| c.as_dictionary());
    let int = |k: &str| cfg.and_then(|c| c.get(k)).and_then(|v| v.as_unsigned_integer().or(v.as_signed_integer().map(|i| i as u64)));
    Rtcp {
        port: int("SourcePort").unwrap_or(0) as u16,
        ours: int("RemoteSSRC").map(|v| v as u32).unwrap_or(fallback_ours),
        phone: int("LocalSSRC").unwrap_or(0) as u32,
    }
}

pub async fn start(adapter: &mut AdapterHandle, rsd: &mut RsdHandshake) -> Result<Screen, String> {
    let mut client = DisplayServiceClient::connect_rsd(adapter, rsd)
        .await
        .map_err(|e| format!("display service: {e:?}"))?;
    let audio = adapter.bind_udp(0).await.map_err(|e| e.to_string())?;
    let video = adapter.bind_udp(0).await.map_err(|e| e.to_string())?;
    let receiver = adapter.host_ip().to_string();
    let sender = adapter.peer_ip().to_string();

    // The strings mirror an offer from Xcode's Device Hub the phone accepts.
    let call_info = CallInfoBlob {
        call_id: 0,
        client_version: 1,
        device_type: "Mac17,7".into(),
        framework_version: "2205.3.1".into(),
        os_version: "25F71".into(),
        device_name: None,
        audio_device_uid: None,
    };
    let session = uuid::Uuid::new_v4();

    let audio_call = uuid::Uuid::new_v4().to_string().to_uppercase();
    let offer = build_screen_audio_offer(&audio_call, &call_info).map_err(|e| format!("{e:?}"))?;
    let params = build_start_audio_parameters(
        &receiver,
        audio.local_port(),
        &sender,
        50000,
        offer,
        CLIENT_SUPPORTED_FEATURES,
        session,
    );
    let audio_answer = client
        .start_media_stream(params)
        .await
        .map_err(|e| format!("audio stream: {e:?}"))?;

    let call = uuid::Uuid::new_v4().to_string().to_uppercase();
    let our_ssrc = uuid::Uuid::new_v4().as_u128() as u32;
    let offer = build_screen_video_offer(&call, &call_info, our_ssrc).map_err(|e| format!("{e:?}"))?;
    let params = build_start_video_parameters(
        &receiver,
        video.local_port(),
        &sender,
        50001,
        offer,
        CLIENT_SUPPORTED_FEATURES,
        1,
        session,
    );
    let answer = client
        .start_media_stream(params)
        .await
        .map_err(|e| format!("video stream: {e:?}"))?;
    tracing::debug!("video answer: {answer:?}");
    let audio_rtcp = rtcp(&audio_answer, 0);
    let video_rtcp = rtcp(&answer, our_ssrc);
    tracing::info!("rtcp: video {video_rtcp:?}, audio {audio_rtcp:?}");
    Ok(Screen { client, audio, video, audio_rtcp, video_rtcp })
}
