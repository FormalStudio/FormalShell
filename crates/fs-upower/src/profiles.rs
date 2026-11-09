use fs_dbus::{Props, Watched, prop};
use futures_lite::Stream;
use zbus::message::Type;
use zbus::proxy::CacheProperties;
use zbus::{Connection, MatchRule, proxy};

const SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";

#[proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles",
    gen_blocking = false
)]
trait Daemon {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<Props>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Self::PowerSaver, Self::Balanced, Self::Performance];

    /// The words every surface shows for the profile.
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerSaver => "Power saver",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
        }
    }

    pub fn from_wire(wire: &str) -> Option<Self> {
        match wire {
            "power-saver" => Some(Self::PowerSaver),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            _ => None,
        }
    }

    pub fn to_wire(self) -> &'static str {
        match self {
            Self::PowerSaver => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilesState {
    pub active: Profile,
    /// In the order the daemon lists them. `Performance` is absent on
    /// hardware that does not offer it.
    pub available: Vec<Profile>,
}

impl ProfilesState {
    pub fn has_performance(&self) -> bool {
        self.available.contains(&Profile::Performance)
    }
}

#[derive(Clone)]
pub struct PowerProfiles {
    conn: Connection,
    daemon: DaemonProxy<'static>,
}

impl PowerProfiles {
    pub async fn connect(conn: &Connection) -> zbus::Result<Self> {
        Ok(Self {
            conn: conn.clone(),
            daemon: DaemonProxy::builder(conn).cache_properties(CacheProperties::No).build().await?,
        })
    }

    pub async fn state(&self) -> zbus::Result<ProfilesState> {
        read_state(&self.daemon).await
    }

    /// Refuses `Performance` locally when the daemon does not list it, instead
    /// of surfacing the bus error.
    pub async fn set_active(&self, profile: Profile) -> zbus::Result<()> {
        if profile == Profile::Performance && !self.state().await?.has_performance() {
            return Err(zbus::Error::Failure(
                "the daemon does not offer the performance profile".into(),
            ));
        }
        self.daemon.set_active_profile(profile.to_wire()).await
    }

    /// The current state, then the new one after every change of
    /// `ActiveProfile` or `Profiles`. None while power-profiles-daemon is not
    /// on the bus, and a whole read again when it returns.
    pub fn changes(&self) -> impl Stream<Item = Option<ProfilesState>> + use<> {
        fs_dbus::watch(self.conn.clone(), Watch(self.clone()))
    }
}

struct Watch(PowerProfiles);

impl Watched for Watch {
    type State = ProfilesState;
    type View = ProfilesState;
    const SERVICE: &'static str = SERVICE;

    fn rule(&self) -> zbus::Result<MatchRule<'static>> {
        Ok(MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)?
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .build())
    }

    async fn read(&self) -> zbus::Result<ProfilesState> {
        self.0.state().await
    }

    fn view(&self, state: &ProfilesState) -> ProfilesState {
        state.clone()
    }
}

async fn read_state(daemon: &DaemonProxy<'_>) -> zbus::Result<ProfilesState> {
    let active = daemon.active_profile().await?;
    let active = Profile::from_wire(&active)
        .ok_or_else(|| zbus::Error::Failure(format!("unknown power profile {active:?}")))?;
    let available = daemon
        .profiles()
        .await?
        .iter()
        .filter_map(|entry| prop::<String>(entry, "Profile"))
        .filter_map(|name| Profile::from_wire(&name))
        .collect();
    Ok(ProfilesState { active, available })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_round_trip() {
        for p in [Profile::PowerSaver, Profile::Balanced, Profile::Performance] {
            assert_eq!(Profile::from_wire(p.to_wire()), Some(p));
        }
        assert_eq!(Profile::from_wire("turbo"), None);
    }

    #[test]
    fn performance_is_only_there_when_listed() {
        let s = ProfilesState {
            active: Profile::Balanced,
            available: vec![Profile::PowerSaver, Profile::Balanced],
        };
        assert!(!s.has_performance());
    }
}
