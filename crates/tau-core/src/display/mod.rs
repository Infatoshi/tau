mod markdown;
mod tables;
mod tools;

use std::io::Write;

use tau_llm::ToolCall;

pub trait AgentDisplay: Send {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()>;
    fn assistant_done(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn tool_call(&mut self, call: &ToolCall) -> anyhow::Result<()>;
    fn tool_result(&mut self, call: &ToolCall, content: &str, is_error: bool)
        -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct StdoutDisplay {
    assistant_buffer: String,
}

impl AgentDisplay for StdoutDisplay {
    fn assistant_delta(&mut self, text: &str) -> anyhow::Result<()> {
        self.assistant_buffer.push_str(text);
        std::io::stdout().flush()?;
        Ok(())
    }

    fn assistant_done(&mut self) -> anyhow::Result<()> {
        if markdown::looks_like_markdown(&self.assistant_buffer) {
            print!("{}", markdown::render_markdown(&self.assistant_buffer));
        } else if !self.assistant_buffer.is_empty() {
            print!("{}", self.assistant_buffer);
        }
        self.assistant_buffer.clear();
        std::io::stdout().flush()?;
        Ok(())
    }

    fn tool_call(&mut self, call: &ToolCall) -> anyhow::Result<()> {
        self.assistant_done()?;
        tools::print_tool_call(call);
        Ok(())
    }

    fn tool_result(
        &mut self,
        call: &ToolCall,
        content: &str,
        is_error: bool,
    ) -> anyhow::Result<()> {
        tools::print_tool_result(call, content, is_error);
        Ok(())
    }
}

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GRAY: &str = "\x1b[90m";

    pub fn dim(text: &str) -> String {
        format!("{DIM}{text}{RESET}")
    }
}
