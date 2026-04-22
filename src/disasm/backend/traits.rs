use crate::disasm::types::DecodedInstruction;

pub trait DisassemblerBackend {
    fn kind(&self) -> super::BackendKind;
    fn name(&self) -> &'static str;
    fn max_instruction_bytes(&self) -> usize;
    fn decode_one(
        &self,
        address: u64,
        bytes: &[u8],
    ) -> crate::error::HxResult<Option<DecodedInstruction>>;
}
