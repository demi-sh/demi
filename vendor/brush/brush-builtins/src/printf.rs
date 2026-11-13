use clap::Parser;
use std::{ffi::OsString, io::Write, ops::ControlFlow};
use uucore::format;

use brush_core::{
    Error, ErrorKind, ExecutionExitCode, ExecutionResult, builtins, escape, expansion,
};

/// Format a string.
#[derive(Parser)]
#[clap(disable_help_flag = true, disable_version_flag = true)]
pub(crate) struct PrintfCommand {
    /// If specified, the output of the command is assigned to this variable.
    #[arg(short = 'v')]
    output_variable: Option<String>,

    /// Format string + arguments to the format string.
    #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
    format_and_args: Vec<String>,
}

impl builtins::Command for PrintfCommand {
    type Error = brush_core::Error;

    async fn execute<SE: brush_core::ShellExtensions>(
        &self,
        context: brush_core::ExecutionContext<'_, SE>,
    ) -> Result<ExecutionResult, Self::Error> {
        let invocation = invocation(&context)?;
        let utf8 = context.shell.utf8_locale();
        let status;
        if let Some(variable_name) = &self.output_variable {
            // Format to a u8 vector.
            let mut result: Vec<u8> = vec![];
            status = match run_in(invocation, &context, || {
                format(self.format_and_args.as_slice(), utf8, &mut result)
            })? {
                ControlFlow::Continue(status) => status,
                ControlFlow::Break(ended) => return Ok(ended.into()),
            };

            // The shell's text, holding the bytes that are no UTF-8.
            let result_str = brush_core::bytes::decode(&result);

            // Assign to the selected variable.
            expansion::assign_to_named_parameter(
                context.shell,
                &context.params,
                variable_name,
                result_str,
            )
            .await?;
        } else {
            // The bytes formatting gives, which are no text: the stream itself.
            let mut stdout = context
                .try_fd(brush_core::openfiles::OpenFiles::STDOUT_FD)
                .ok_or_else(|| std::io::Error::other("standard output not available"))?;
            status = match run_in(invocation, &context, || {
                format(self.format_and_args.as_slice(), utf8, &mut stdout)
            })? {
                ControlFlow::Continue(status) => status,
                ControlFlow::Break(ended) => return Ok(ended.into()),
            };
            stdout.flush()?;
        }

        Ok(status.into())
    }
}

/// The invocation uucore's formatting runs in, as an embedded utility's does:
/// it reads the locale from the shell's exported variables (`%q`) and writes
/// its warnings (`printf %d abc`) to printf's standard error through the
/// host's control, which records a write to a file, and has nothing to read
/// or write them through outside one.
fn invocation<SE: brush_core::ShellExtensions>(
    context: &brush_core::ExecutionContext<'_, SE>,
) -> Result<uucore::context::Context, brush_core::Error> {
    // A stream the command closed (`printf x 2>&-`) is the null device, which
    // takes what is written and gives nothing to read.
    let stream = |descriptor| -> Result<std::sync::Arc<std::fs::File>, brush_core::Error> {
        let file = match context.try_fd(descriptor) {
            Some(file) => file.into_file()?,
            None => std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(if cfg!(windows) { "NUL" } else { "/dev/null" })?,
        };
        Ok(std::sync::Arc::new(file))
    };
    let shell = &*context.shell;
    Ok(uucore::context::Context {
        name: "printf",
        control: shell
            .execution_host()
            .and_then(|host| host.utility_control(shell.child_attributes())),
        // printf creates no file: the mask is the shell's, or none beyond the
        // process's own.
        umask: shell.child_attributes().umask.unwrap_or(0),
        cwd: shell.working_dir().to_owned(),
        env: shell
            .env()
            .iter_exported()
            .filter(|(_, variable)| variable.value().is_set())
            .map(|(name, variable)| {
                let value = brush_core::bytes::to_os(&variable.value().to_cow_str(shell)).into_owned();
                (name.into(), value)
            })
            .collect(),
        descriptors: std::collections::BTreeMap::new(),
        stdin: stream(0)?,
        stdout: stream(1)?,
        stderr: stream(2)?,
    })
}

/// Runs `work` in `invocation` and gives printf's status, 1 when an argument
/// was no number, as bash's: printf prints what it can of the rest. Breaks
/// with the status uucore ends printf with as it ends a utility: 141 when a
/// diagnostic meets a pipe whose reader has gone, as SIGPIPE ends a program;
/// and fails with the shell's cancellation once its job is cancelled.
fn run_in<SE: brush_core::ShellExtensions>(
    invocation: uucore::context::Context,
    context: &brush_core::ExecutionContext<'_, SE>,
    work: impl FnOnce() -> Result<(), brush_core::Error>,
) -> Result<ControlFlow<ExecutionExitCode, ExecutionExitCode>, brush_core::Error> {
    let ended = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        uucore::context::with(invocation, || {
            // uucore keeps the status on the thread, which ran others before.
            uucore::error::set_exit_code(0);
            work()?;
            Ok(uucore::error::get_exit_code())
        })
    })) {
        Ok(result) => {
            return result.map(|status: i32| ControlFlow::Continue((status as u8).into()));
        }
        Err(ended) => ended,
    };
    if let Some(request) = ended.downcast_ref::<uucore::context::ExitRequest>() {
        return Ok(ControlFlow::Break((request.0 as u8).into()));
    }
    if ended.is::<uucore::context::Cancelled>() {
        context.shell.check_execution()?;
        return Err(std::io::Error::other("shell job cancelled").into());
    }
    std::panic::resume_unwind(ended)
}

fn format(
    format_and_args: &[String],
    utf8: bool,
    writer: impl Write,
) -> Result<(), brush_core::Error> {
    // uucore formats bytes: the format and the arguments as the bytes they
    // stand for (`bytes::encode`), so a held byte is one byte to `%c`, `%.1s`
    // and a width.
    match format_and_args {
        [fmt, args @ ..] => format_via_uucore(
            &brush_core::bytes::encode(fmt),
            args.iter().map(|arg| brush_core::bytes::to_os(arg).into_owned()),
            utf8,
            writer,
        ),
        // Handle case with no format string (we shouldn't be able to get here since clap will
        // fail parsing when the format string is missing)
        [] => Err(ErrorKind::PrintfInvalidUsage("missing operand".into()).into()),
    }
}

fn format_via_uucore(
    format_string: &[u8],
    args: impl Iterator<Item = impl Into<OsString>>,
    utf8: bool,
    mut writer: impl Write,
) -> Result<(), brush_core::Error> {
    // Convert string arguments to FormatArgument::Unparsed
    let format_args: Vec<_> = args
        .map(|s| format::FormatArgument::Unparsed(s.into()))
        .collect();

    // Parse format string once.
    let format_items = parse_format_string(format_string)?;

    // Wrap the format arguments.
    let mut format_args_wrapper = format::FormatArguments::as_bash_reads(&format_args);

    // Keep going until we've exhausted all format arguments. Also make sure to run at least once
    // even if there's no format arguments.
    while format_args.is_empty() || !format_args_wrapper.is_exhausted() {
        // Process all format items, in order. We'll bail when we're told to stop.
        for item in &format_items {
            // %q quotes as bash's does, with backslashes in the shell's locale
            // (bash-completion reads them), where uucore quotes as GNU's printf does.
            if let format::FormatItem::Spec(format::Spec::QuotedString { position }) = item {
                let word = brush_core::bytes::from_os(format_args_wrapper.next_string(*position));
                let quoted =
                    escape::quote_if_needed_in(&word, escape::QuoteMode::BackslashEscape, utf8);
                writer.write_all(&brush_core::bytes::encode(&quoted))?;
                continue;
            }
            let control_flow = item
                .write(&mut writer, &mut format_args_wrapper)
                .map_err(|e| match e {
                    // Propagate I/O errors directly so they can be handled appropriately
                    format::FormatError::IoError(io_err) => Error::from(io_err),
                    // Wrap other format errors
                    other => Error::from(ErrorKind::PrintfInvalidUsage(std::format!(
                        "printf formatting error: {other}"
                    ))),
                })?;

            if control_flow == ControlFlow::Break(()) {
                break;
            }
        }

        // Start next batch if not exhausted
        if !format_args_wrapper.is_exhausted() {
            format_args_wrapper.start_next_batch();
        }

        if format_args.is_empty() {
            break;
        }
    }

    Ok(())
}

fn parse_format_string(
    format_string: &[u8],
) -> Result<Vec<format::FormatItem<format::EscapedChar>>, brush_core::Error> {
    let format_items: Result<Vec<_>, _> = format::parse_spec_and_escape(format_string).collect();

    // Observe any errors we encountered along the way.
    let format_items = format_items
        .map_err(|e| ErrorKind::PrintfInvalidUsage(format!("printf parsing error: {e}")))?;

    Ok(format_items)
}

#[cfg(test)]
#[expect(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn sprintf_via_uucore(
        format_string: &str,
        args: impl Iterator<Item = impl Into<OsString>>,
    ) -> Result<String> {
        let mut result = vec![];
        format_via_uucore(format_string.as_bytes(), args, true, &mut result)?;

        Ok(String::from_utf8(result)?)
    }

    #[test]
    fn test_basic_sprintf() -> Result<()> {
        assert_eq!(sprintf_via_uucore("%s", std::iter::once(&"xyz"))?, "xyz");
        assert_eq!(sprintf_via_uucore(r"%d\n", std::iter::once(&"1"))?, "1\n");

        Ok(())
    }

    #[test]
    fn test_sprintf_without_args() -> Result<()> {
        let empty: [&str; 0] = [];

        assert_eq!(sprintf_via_uucore("xyz", empty.iter())?, "xyz");
        assert_eq!(sprintf_via_uucore("%s|", empty.iter())?, "|");

        Ok(())
    }

    #[test]
    fn test_sprintf_with_cycles() -> Result<()> {
        assert_eq!(sprintf_via_uucore("%s|", ["x", "y"].iter())?, "x|y|");

        Ok(())
    }
}
