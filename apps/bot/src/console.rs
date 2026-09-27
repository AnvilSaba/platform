use std::{
    io::{self, BufRead as _, IsTerminal as _, Write as _},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, Result};
use rustyline::{
    Context, Editor, ExternalPrinter, Helper,
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::DefaultHistory,
    validate::Validator,
};
use serenity::{http::Http, model::application::Command};
use tokio::{sync::mpsc, time::sleep};
use tracing::error;

use crate::features::commands;

#[derive(Default)]
pub struct ConsoleOutput {
    printer: Mutex<Option<Box<dyn ExternalPrinter + Send>>>,
}

impl ConsoleOutput {
    fn set_printer(&self, printer: Box<dyn ExternalPrinter + Send>) {
        *self.printer.lock().expect("console printer lock poisoned") = Some(printer);
    }

    fn clear_printer(&self) {
        *self.printer.lock().expect("console printer lock poisoned") = None;
    }

    fn print_log(&self, message: String) {
        if let Ok(mut guard) = self.printer.lock()
            && let Some(printer) = guard.as_mut()
            && printer.print(message.clone()).is_ok()
        {
            return;
        }
        let _ = io::stdout().write_all(message.as_bytes());
    }
}

pub fn interactive_terminal() -> bool {
    let unsupported = std::env::var("TERM")
        .map(|term| {
            ["dumb", "cons25", "emacs"]
                .iter()
                .any(|name| term.eq_ignore_ascii_case(name))
        })
        .unwrap_or(false);
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !unsupported
}

pub struct LogWriter {
    output: Arc<ConsoleOutput>,
}

impl LogWriter {
    pub fn new(output: Arc<ConsoleOutput>) -> Self {
        Self { output }
    }
}

impl io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.print_log(String::from_utf8_lossy(bytes).into_owned());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ConsoleHelper {
    deletable_names: Vec<String>,
}

impl ConsoleHelper {
    fn new() -> Self {
        let mut deletable_names = Vec::new();
        for command in commands() {
            deletable_names.push(command.name.to_string());
            if let Some(name) = command.context_menu_name {
                deletable_names.push(name.to_string());
            }
        }
        deletable_names.sort();
        deletable_names.dedup();
        Self { deletable_names }
    }

    fn candidates(&self, line: &str, pos: usize) -> (usize, Vec<Pair>) {
        let Some(prefix) = line.get(..pos) else {
            return (pos, Vec::new());
        };
        if let Some(fragment) = prefix.strip_prefix("delete ") {
            let candidates = self
                .deletable_names
                .iter()
                .filter(|choice| choice.starts_with(fragment))
                .map(|choice| Pair {
                    display: choice.clone(),
                    replacement: choice.clone(),
                })
                .collect();
            ("delete ".len(), candidates)
        } else if prefix.contains(char::is_whitespace) {
            (pos, Vec::new())
        } else {
            let candidates = ["register", "list", "delete-all", "delete ", "help"]
                .into_iter()
                .filter(|choice| choice.starts_with(prefix))
                .map(|choice| Pair {
                    display: choice.trim_end().to_owned(),
                    replacement: choice.to_owned(),
                })
                .collect();
            (0, candidates)
        }
    }
}

impl Completer for ConsoleHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        Ok(self.candidates(line, pos))
    }
}

impl Hinter for ConsoleHelper {
    type Hint = String;
}

impl Highlighter for ConsoleHelper {}
impl Validator for ConsoleHelper {}
impl Helper for ConsoleHelper {}

enum ConsoleInput {
    Line(String, std::sync::mpsc::Sender<()>),
    Interrupt,
}

fn send_line(sender: &mpsc::Sender<ConsoleInput>, line: String) -> bool {
    let (acknowledge, completed) = std::sync::mpsc::channel();
    sender.blocking_send(ConsoleInput::Line(line, acknowledge)).is_ok() && completed.recv().is_ok()
}

fn read_terminal_lines(sender: &mpsc::Sender<ConsoleInput>, output: &ConsoleOutput) -> rustyline::Result<()> {
    let mut editor = Editor::<ConsoleHelper, DefaultHistory>::new()?;
    editor.set_helper(Some(ConsoleHelper::new()));
    output.set_printer(Box::new(editor.create_external_printer()?));

    let result = loop {
        match editor.readline("> ") {
            Ok(line) => {
                let _ = editor.add_history_entry(line.as_str());
                if !send_line(sender, line) {
                    break Ok(());
                }
            }
            Err(ReadlineError::Interrupted) => {
                let _ = sender.blocking_send(ConsoleInput::Interrupt);
                break Ok(());
            }
            Err(ReadlineError::Eof) => continue,
            Err(error) => break Err(error),
        }
    };

    output.clear_printer();
    result
}

fn read_plain_lines(sender: &mpsc::Sender<ConsoleInput>) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(line) => {
                if !send_line(sender, line) {
                    break;
                }
            }
            Err(error) => {
                error!("標準入力を読み取れませんでした: {error}");
                break;
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ConsoleCommand<'a> {
    Register,
    List,
    DeleteAll,
    Delete(&'a str),
    Help,
    Unknown,
}

fn parse_command(line: &str) -> ConsoleCommand<'_> {
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next(), parts.next()) {
        (Some("register"), None, None) => ConsoleCommand::Register,
        (Some("list"), None, None) => ConsoleCommand::List,
        (Some("delete-all"), None, None) => ConsoleCommand::DeleteAll,
        (Some("delete"), Some(name), None) => ConsoleCommand::Delete(name),
        (Some("help"), None, None) => ConsoleCommand::Help,
        _ => ConsoleCommand::Unknown,
    }
}

async fn wait_for_application_id(http: &Http) {
    while http.application_id().is_none() {
        sleep(Duration::from_millis(100)).await;
    }
}

pub async fn run<F>(http: Arc<Http>, shutdown: F, output: Arc<ConsoleOutput>)
where
    F: FnOnce() -> bool + Send + 'static,
{
    wait_for_application_id(&http).await;

    let (sender, lines) = mpsc::channel(32);
    std::thread::spawn(move || {
        if interactive_terminal() {
            if let Err(error) = read_terminal_lines(&sender, &output) {
                error!("対話入力を開始できませんでした: {error}");
            }
        } else {
            read_plain_lines(&sender);
        }
    });

    process_inputs(lines, &http, shutdown).await;
}

async fn execute_command(command: ConsoleCommand<'_>, http: &Http) -> Result<()> {
    match command {
        ConsoleCommand::Register => {
            poise::builtins::register_globally(http, &commands())
                .await
                .context("コマンドを登録できませんでした")?;
            println!("Discord のグローバルコマンドを登録しました。");
        }

        ConsoleCommand::List => {
            let mut commands = Command::get_global_commands(http)
                .await
                .context("登録済みコマンドを取得できませんでした")?;

            if commands.is_empty() {
                println!("登録済みのグローバルコマンドはありません。");
            } else {
                commands.sort_by(|a, b| a.name.cmp(&b.name));
                for command in commands {
                    println!("{} - {}", command.name, command.id);
                }
            }
        }

        ConsoleCommand::DeleteAll => {
            Command::set_global_commands(http, &[])
                .await
                .context("コマンドを削除できませんでした")?;
            println!("Discord のグローバルコマンドをすべて削除しました。");
        }

        ConsoleCommand::Delete(name) => {
            let commands = Command::get_global_commands(http)
                .await
                .context("登録済みコマンドを取得できませんでした")?;

            let matches: Vec<_> = commands.into_iter().filter(|command| command.name == name).collect();
            if matches.is_empty() {
                println!("コマンド {name} は登録されていません。");
            }

            for command in matches {
                Command::delete_global_command(http, command.id)
                    .await
                    .with_context(|| format!("コマンド {} を削除できませんでした", command.name))?;
                println!("コマンド {} を削除しました。", command.name);
            }
        }

        ConsoleCommand::Help => {
            println!("register: 定義済みコマンドを登録（グローバルコマンドを置換）");
            println!("list: 登録済みのグローバルコマンドを表示");
            println!("delete <名前>: 指定した名前のグローバルコマンドを削除");
            println!("delete-all: グローバルコマンドをすべて削除");
        }

        ConsoleCommand::Unknown => println!("不明なコマンドです。help で使い方を表示します。"),
    }

    Ok(())
}

async fn process_inputs<F>(mut lines: mpsc::Receiver<ConsoleInput>, http: &Http, shutdown: F)
where
    F: FnOnce() -> bool,
{
    while let Some(input) = lines.recv().await {
        let ConsoleInput::Line(line, acknowledge) = input else {
            shutdown();
            break;
        };

        if let Err(error) = execute_command(parse_command(&line), http).await {
            error!("{error:#}");
        }

        let _ = acknowledge.send(());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write as _,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use rustyline::ExternalPrinter;
    use serenity::{http::Http, model::id::ApplicationId};
    use tokio::sync::mpsc;

    use super::{
        ConsoleCommand, ConsoleHelper, ConsoleInput, ConsoleOutput, LogWriter, parse_command, process_inputs,
        wait_for_application_id,
    };

    struct RecordingPrinter(Arc<Mutex<Vec<String>>>);

    impl ExternalPrinter for RecordingPrinter {
        fn print(&mut self, message: String) -> rustyline::Result<()> {
            self.0.lock().unwrap().push(message);
            Ok(())
        }
    }

    #[test]
    fn parses_console_commands() {
        assert_eq!(parse_command(" register "), ConsoleCommand::Register);
        assert_eq!(parse_command("list"), ConsoleCommand::List);
        assert_eq!(parse_command("list extra"), ConsoleCommand::Unknown);
        assert_eq!(parse_command("delete pin"), ConsoleCommand::Delete("pin"));
        assert_eq!(parse_command("delete-all"), ConsoleCommand::DeleteAll);
        assert_eq!(parse_command("delete all"), ConsoleCommand::Delete("all"));
        assert_eq!(parse_command("delete"), ConsoleCommand::Unknown);
        assert_eq!(parse_command("register extra"), ConsoleCommand::Unknown);
    }

    #[test]
    fn completes_commands_and_delete_targets() {
        let helper = ConsoleHelper::new();
        let (start, candidates) = helper.candidates("reg", 3);
        assert_eq!(start, 0);
        assert_eq!(candidates[0].replacement, "register");

        let (_, candidates) = helper.candidates("li", 2);
        assert_eq!(candidates[0].replacement, "list");

        let (start, candidates) = helper.candidates("delete pi", "delete pi".len());
        assert_eq!(start, "delete ".len());
        assert!(candidates.iter().any(|candidate| candidate.replacement == "pin"));

        let (_, candidates) = helper.candidates("delete-a", "delete-a".len());
        assert!(candidates.iter().any(|candidate| candidate.replacement == "delete-all"));
    }

    #[test]
    fn forwards_logs_through_the_line_editor_printer() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let output = Arc::new(ConsoleOutput::default());
        output.set_printer(Box::new(RecordingPrinter(recorded.clone())));

        let mut writer = LogWriter::new(output);
        writer.write_all(b"bot connected\n").unwrap();

        assert_eq!(*recorded.lock().unwrap(), ["bot connected\n"]);
    }

    #[tokio::test]
    async fn waits_for_gateway_application_id() {
        let http = Arc::new(Http::without_token());
        let ready_http = http.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            ready_http.set_application_id(ApplicationId::new(42));
        });

        wait_for_application_id(&http).await;
        assert_eq!(http.application_id(), Some(ApplicationId::new(42)));
    }

    #[tokio::test]
    async fn console_interrupt_requests_bot_shutdown() {
        let (sender, receiver) = mpsc::channel(1);
        sender.send(ConsoleInput::Interrupt).await.unwrap();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let flag = shutdown_requested.clone();

        process_inputs(receiver, &Http::without_token(), move || {
            flag.store(true, Ordering::SeqCst);
            true
        })
        .await;

        assert!(shutdown_requested.load(Ordering::SeqCst));
    }
}
