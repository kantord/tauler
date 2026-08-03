#[derive(serde::Serialize, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// 0=low 1=normal 2=critical
    pub urgency: u8,
    pub enwiro_env: Option<String>,
}

/// Why a notification went away, as reported in the `reason` argument of the
/// freedesktop `NotificationClosed` signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Expired,
    Dismissed,
    Closed,
}

impl From<CloseReason> for u32 {
    /// The numbers are the spec's wire values, not an internal choice.
    fn from(reason: CloseReason) -> u32 {
        match reason {
            CloseReason::Expired => 1,
            CloseReason::Dismissed => 2,
            CloseReason::Closed => 3,
        }
    }
}

pub enum Event {
    Add(Notification, i32 /* expire_timeout from spec */),
    /// A timer firing, implying `CloseReason::Expired`. Only removes the entry
    /// if `generation` still matches, so a superseded notification's timer
    /// cannot close its replacement.
    Expire {
        id: u32,
        generation: u64,
    },
    /// A dismissal or CloseNotification call: removes whatever holds the id.
    /// The reason distinguishes the two for the signal we then emit.
    Close {
        id: u32,
        reason: CloseReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // The freedesktop notification spec fixes these numbers on the wire, in
    // the `reason` argument of NotificationClosed. Renumbering them silently
    // lies to every client, so the values are asserted literally.
    #[test]
    fn expiry_is_reason_one() {
        assert_eq!(u32::from(CloseReason::Expired), 1);
    }

    #[test]
    fn user_dismissal_is_reason_two() {
        assert_eq!(u32::from(CloseReason::Dismissed), 2);
    }

    #[test]
    fn close_notification_call_is_reason_three() {
        assert_eq!(u32::from(CloseReason::Closed), 3);
    }
}
