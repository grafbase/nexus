pub mod chat;

pub trait Error: Send + 'static {
    fn error_type(&self) -> &str;
}
