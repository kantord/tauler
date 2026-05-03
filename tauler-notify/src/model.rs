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

pub enum Event {
    Add(Notification, i32 /* expire_timeout from spec */),
    Remove(u32),
}
