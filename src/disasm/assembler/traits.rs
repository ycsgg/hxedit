use crate::error::HxResult;
use crate::executable::{Bitness, Endian, ExecutableArch};

pub trait AssemblerBackend {
    fn kind(&self) -> super::AssemblerKind;
    fn name(&self) -> &'static str;
    fn supports_arch(&self, arch: ExecutableArch, bitness: Bitness, endian: Endian) -> bool;
    fn assemble_one(
        &self,
        arch: ExecutableArch,
        bitness: Bitness,
        endian: Endian,
        address: u64,
        statement: &str,
    ) -> HxResult<Vec<u8>>;
}
