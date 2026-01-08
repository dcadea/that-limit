use crate::cfg;

#[derive(Debug)]
pub enum Error {
    Cfg(cfg::Error),
    Io(std::io::Error),
}

impl From<cfg::Error> for Error {
    fn from(e: cfg::Error) -> Self {
        Self::Cfg(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
