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

    /// Decode a run of consecutive instructions from a single contiguous buffer.
    ///
    /// Decoding stops at the first byte that does not decode to a non-empty
    /// instruction, after `max_instructions` have been produced, or when the
    /// buffer is exhausted. `address` is the virtual address of `bytes[0]`.
    ///
    /// The default implementation repeatedly calls [`decode_one`]; backends with
    /// a streaming decoder (e.g. iced-x86) override this to reuse decoder state
    /// across the whole buffer.
    fn decode_block(
        &self,
        address: u64,
        bytes: &[u8],
        max_instructions: usize,
        out: &mut Vec<DecodedInstruction>,
    ) -> crate::error::HxResult<()> {
        let mut pos = 0usize;
        while out.len() < max_instructions && pos < bytes.len() {
            match self.decode_one(address.wrapping_add(pos as u64), &bytes[pos..])? {
                Some(decoded) if !decoded.bytes.is_empty() => {
                    pos += decoded.bytes.len();
                    out.push(decoded);
                }
                _ => break,
            }
        }
        Ok(())
    }
}
