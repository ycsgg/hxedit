use super::*;
use crate::commands::types::AnalysisCommand;

impl App {
    pub(super) fn execute_analysis_command(&mut self, command: AnalysisCommand) -> HxResult<()> {
        match command {
            AnalysisCommand::Run => self.start_sagitta_analysis(),
            AnalysisCommand::Status => {
                self.set_info_status(self.sagitta_analysis_status_message());
                Ok(())
            }
            AnalysisCommand::Off => {
                self.clear_sagitta_analysis();
                Ok(())
            }
        }
    }
}
