use std::{
    error::Error,
    io::{self},
    str::FromStr,
    sync::Arc,
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tokio::sync::RwLock;

use crate::{
    crypto::{self},
    KvStoreTxPool, State, Storage, Transaction, TransactionKind, TransactionWithAccount,
    UnsignedTransaction,
};

#[derive(PartialEq, Eq)]
enum ActiveInput {
    SendTxPrivateKey,
    SendTxKey,
    SendTxValue,
    QueryAccount,
    QueryKey,
}

struct App {
    state: Arc<RwLock<State>>,
    storage: Arc<dyn Storage>,
    mempool: KvStoreTxPool,
    tabs: Vec<&'static str>,
    tab_index: usize,

    active_input: ActiveInput,

    // Send Transaction tab state
    send_tx_inputs: [String; 3], // 0: private_key, 1: key, 2: value
    send_tx_result: String,

    // Query Value tab state
    query_value_inputs: [String; 2], // 0: account_address, 1: key
    query_value_result: String,
}

impl App {
    fn new(
        state: Arc<RwLock<State>>,
        storage: Arc<dyn Storage>,
        mempool: KvStoreTxPool,
    ) -> Self {
        let keypair = crypto::generate_keypair();
        let address = crypto::public_key_to_address(&keypair.public_key);
        let mut send_tx_inputs = [String::new(), String::new(), String::new()];
        send_tx_inputs[0] = hex::encode(keypair.secret_key.secret_bytes());

        let mut query_value_inputs = [String::new(), String::new()];
        query_value_inputs[0] = address;

        Self {
            state,
            storage,
            mempool,
            tabs: vec!["Explorer", "Send Transaction", "Query Value"],
            tab_index: 0,
            active_input: ActiveInput::SendTxKey,
            send_tx_inputs,
            send_tx_result: String::new(),
            query_value_inputs,
            query_value_result: String::new(),
        }
    }

    pub fn next_tab(&mut self) {
        self.tab_index = (self.tab_index + 1) % self.tabs.len();
        self.update_active_input_for_tab();
    }

    pub fn previous_tab(&mut self) {
        if self.tab_index > 0 {
            self.tab_index -= 1;
        } else {
            self.tab_index = self.tabs.len() - 1;
        }
        self.update_active_input_for_tab();
    }

    fn update_active_input_for_tab(&mut self) {
        match self.tab_index {
            1 => self.active_input = ActiveInput::SendTxPrivateKey,
            2 => self.active_input = ActiveInput::QueryAccount,
            _ => {}
        }
    }

    fn next_input(&mut self) {
        match self.tab_index {
            1 => {
                self.active_input = match self.active_input {
                    ActiveInput::SendTxPrivateKey => ActiveInput::SendTxKey,
                    ActiveInput::SendTxKey => ActiveInput::SendTxValue,
                    ActiveInput::SendTxValue => ActiveInput::SendTxPrivateKey,
                    _ => ActiveInput::SendTxPrivateKey,
                }
            }
            2 => {
                self.active_input = match self.active_input {
                    ActiveInput::QueryAccount => ActiveInput::QueryKey,
                    ActiveInput::QueryKey => ActiveInput::QueryAccount,
                    _ => ActiveInput::QueryAccount,
                }
            }
            _ => {}
        }
    }

    fn push_char(&mut self, c: char) {
        match self.active_input {
            ActiveInput::SendTxPrivateKey => self.send_tx_inputs[0].push(c),
            ActiveInput::SendTxKey => self.send_tx_inputs[1].push(c),
            ActiveInput::SendTxValue => self.send_tx_inputs[2].push(c),
            ActiveInput::QueryAccount => self.query_value_inputs[0].push(c),
            ActiveInput::QueryKey => self.query_value_inputs[1].push(c),
        }
    }

    fn pop_char(&mut self) {
        match self.active_input {
            ActiveInput::SendTxPrivateKey => self.send_tx_inputs[0].pop(),
            ActiveInput::SendTxKey => self.send_tx_inputs[1].pop(),
            ActiveInput::SendTxValue => self.send_tx_inputs[2].pop(),
            ActiveInput::QueryAccount => self.query_value_inputs[0].pop(),
            ActiveInput::QueryKey => self.query_value_inputs[1].pop(),
        };
    }

    async fn submit(&mut self) {
        match self.tab_index {
            1 => self.send_transaction().await,
            2 => self.query_value().await,
            _ => {}
        }
    }

    async fn send_transaction(&mut self) {
        let private_key_hex = &self.send_tx_inputs[0];
        let key = &self.send_tx_inputs[1];
        let value = &self.send_tx_inputs[2];

        if key.is_empty() || value.is_empty() {
            self.send_tx_result = "Error: Key and Value cannot be empty".to_string();
            return;
        }

        let private_key_bytes = match hex::decode(private_key_hex) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.send_tx_result = format!("Error: Invalid private key hex: {}", e);
                return;
            }
        };

        let private_key = match SecretKey::from_slice(&private_key_bytes) {
            Ok(pk) => pk,
            Err(e) => {
                self.send_tx_result = format!("Error: Invalid private key: {}", e);
                return;
            }
        };
        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &private_key);
        let address = crypto::public_key_to_address(&public_key);
        
        let unsigned_transaction = UnsignedTransaction {
            nonce: self.state.read().await.get_account(
                &address
            ).map(|s| s.nonce)
            .unwrap_or(0), 
            kind: TransactionKind::SetKV {
                key: key.clone(),
                value: value.clone(),
            },
        };

        let signature = crypto::sign_transaction(&unsigned_transaction, &private_key);

        let transaction = Transaction {
            unsigned: unsigned_transaction,
            signature,
        };

        

        let txn_with_account = TransactionWithAccount {
            txn: transaction,
            address,
        };
        let txn_hash = self.mempool.add_raw_txn(txn_with_account);
        self.send_tx_result = format!("Transaction sent! Hash: {}", hex::encode(txn_hash.0));
    }

    async fn query_value(&mut self) {
        let account_address = &self.query_value_inputs[0];
        let key = &self.query_value_inputs[1];

        if account_address.is_empty() || key.is_empty() {
            self.query_value_result = "Error: Account address and key cannot be empty".to_string();
            return;
        }

        match self.state.read().await.get_account(account_address) {
            Some(account) => match account.kv_store.get(key) {
                Some(value) => self.query_value_result = format!("Value: {}", value),
                None => self.query_value_result = format!("Error: Key not found {}", key),
            },
            None => self.query_value_result = format!("Error: Account not found {}", account_address),
        }
    }
}

fn cleanup_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

pub async fn run_tui(
    state: Arc<RwLock<State>>,
    storage: Arc<dyn Storage>,
    mempool: KvStoreTxPool,
) -> Result<(), Box<dyn Error>> {
    // 设置 panic hook
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        cleanup_terminal();
        original_hook(panic);
    }));

    // 启用原始模式和替代屏幕
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new(state, storage, mempool);
    
    // 使用 tokio::select! 来处理信号
    let res = tokio::select! {
        result = run_app(&mut terminal, app) => result,
        _ = tokio::signal::ctrl_c() => {
            println!("Received Ctrl+C, exiting...");
            Ok(())
        }
    };

    cleanup_terminal();

    if let Err(err) = res {
        eprintln!("Application error: {err:?}");
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::<B>(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Right => app.next_tab(),
                KeyCode::Left => app.previous_tab(),
                KeyCode::Tab => app.next_input(),
                KeyCode::Char(c) => app.push_char(c),
                KeyCode::Backspace => app.pop_char(),
                KeyCode::Enter => app.submit().await,
                _ => {}
            }
        }
    }
}

fn ui<B: Backend>(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(size);

    let titles: Vec<_> = app
        .tabs
        .iter()
        .map(|t| Line::from(Span::styled(*t, Style::default().fg(Color::Green))))
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .select(app.tab_index)
        .style(Style::default().fg(Color::Cyan))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::Black),
        );
    f.render_widget(tabs, chunks[0]);

    match app.tab_index {
        0 => {
            let block = Block::default()
                .title("Explorer (Coming Soon)")
                .borders(Borders::ALL);
            f.render_widget(block, chunks[1]);
        }
        1 => draw_send_transaction_tab::<B>(f, app, chunks[1]),
        2 => draw_query_value_tab::<B>(f, app, chunks[1]),
        _ => {}
    };
}

fn draw_send_transaction_tab<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
            .as_ref(),
        )
        .split(area);

    let inputs = [
        ("Private Key", ActiveInput::SendTxPrivateKey),
        ("Key", ActiveInput::SendTxKey),
        ("Value", ActiveInput::SendTxValue),
    ];

    for i in 0..inputs.len() {
        let (title, input_type) = &inputs[i];
        let text = &app.send_tx_inputs[i];
        let p = Paragraph::new(text.as_str())
            .style(if &app.active_input == input_type {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(Block::default().borders(Borders::ALL).title(*title));
        f.render_widget(p, chunks[i]);
    }

    let result = Paragraph::new(app.send_tx_result.as_str())
        .block(Block::default().borders(Borders::ALL).title("Result"));
    f.render_widget(result, chunks[3]);

    let help = Paragraph::new("Press 'Tab' to switch inputs, 'Enter' to send.")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[4]);
}

fn draw_query_value_tab<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
            .as_ref(),
        )
        .split(area);

    let inputs = [
        ("Account Address", ActiveInput::QueryAccount),
        ("Key", ActiveInput::QueryKey),
    ];

    for i in 0..inputs.len() {
        let (title, input_type) = &inputs[i];
        let text = &app.query_value_inputs[i];
        let p = Paragraph::new(text.as_str())
            .style(if &app.active_input == input_type {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(Block::default().borders(Borders::ALL).title(*title));
        f.render_widget(p, chunks[i]);
    }

    let result = Paragraph::new(app.query_value_result.as_str())
        .block(Block::default().borders(Borders::ALL).title("Result"));
    f.render_widget(result, chunks[2]);

    let help = Paragraph::new("Press 'Tab' to switch inputs, 'Enter' to query.")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[3]);
}
