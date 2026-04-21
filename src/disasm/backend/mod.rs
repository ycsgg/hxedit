#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Capstone,
    IcedX86,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Capstone => "capstone",
            Self::IcedX86 => "iced-x86",
        }
    }
}
