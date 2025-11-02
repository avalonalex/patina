# TUI Implementation Guide

Using `ratatui` and `tui-textarea` to build the Patina notebook interface.

## Dependencies

```toml
[dependencies]
# ... existing deps ...

# TUI framework
ratatui = "0.29"
crossterm = "0.28"

# Multi-line text editing
tui-textarea = "0.6"

# For notebook format
serde = { version = "1.0", features = ["derive"] }
```

## Architecture

```
┌─────────────────────────────────────────┐
│         Notebook Application            │
│  - State management                     │
│  - Event handling                       │
│  - Cell orchestration                   │
└─────────────────────────────────────────┘
                  │
     ┌────────────┼────────────┐
     │            │            │
┌────▼────┐  ┌───▼────┐  ┌───▼────┐
│   UI    │  │  Eval  │  │ Format │
│(ratatui)│  │ Engine │  │ Parser │
└────┬────┘  └────────┘  └────────┘
     │
┌────▼────────┐
│tui-textarea │
│ (editing)   │
└─────────────┘
```

## Core Components

### 1. Notebook State

```rust
use tui_textarea::TextArea;
use ratatui::widgets::*;

pub struct Notebook {
    cells: Vec<Cell>,
    current_cell: usize,
    mode: Mode,
    evaluator: Evaluator,
}

pub struct Cell {
    id: String,
    cell_type: CellType,
    editor: TextArea<'static>,
    output: Option<CellOutput>,
    execution_count: Option<usize>,
    stale: bool,
}

pub enum CellType {
    Code,
    Markdown,
}

pub enum Mode {
    Normal,   // Navigate between cells
    Edit,     // Edit current cell
    Command,  // Execute commands
}

pub struct CellOutput {
    value: String,
    stdout: String,
    stderr: String,
    execution_time: Duration,
}
```

### 2. Event Loop

```rust
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub fn run_notebook(path: &str) -> Result<()> {
    // Setup terminal
    let mut terminal = setup_terminal()?;
    let mut notebook = Notebook::load(path)?;

    loop {
        // Render
        terminal.draw(|f| ui::render(f, &notebook))?;

        // Handle events
        if let Event::Key(key) = event::read()? {
            match notebook.mode {
                Mode::Normal => handle_normal_mode(&mut notebook, key)?,
                Mode::Edit => handle_edit_mode(&mut notebook, key)?,
                Mode::Command => handle_command_mode(&mut notebook, key)?,
            }
        }

        // Check for quit
        if notebook.should_quit {
            break;
        }
    }

    cleanup_terminal(terminal)?;
    Ok(())
}
```

### 3. Cell Rendering

```rust
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

fn render_cell(f: &mut Frame, area: Rect, cell: &Cell, is_selected: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("[{}] {}", cell.execution_count.unwrap_or(0),
                      match cell.cell_type {
                          CellType::Code => "Code",
                          CellType::Markdown => "Markdown",
                      }))
        .border_style(if is_selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split into input and output
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),      // Input area
            Constraint::Length(1),   // Separator
            Constraint::Min(1),      // Output area
        ])
        .split(inner);

    // Render editor using tui-textarea
    f.render_widget(cell.editor.widget(), chunks[0]);

    // Render output if exists
    if let Some(output) = &cell.output {
        render_output(f, chunks[2], output);
    }
}

fn render_output(f: &mut Frame, area: Rect, output: &CellOutput) {
    let style = Style::default().fg(Color::Green);
    let text = vec![
        Line::from(vec![
            Span::styled("=> ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&output.value, style),
        ]),
    ];

    let para = Paragraph::new(text)
        .style(Style::default());
    f.render_widget(para, area);
}
```

### 4. Key Bindings (Normal Mode)

```rust
fn handle_normal_mode(notebook: &mut Notebook, key: KeyEvent) -> Result<()> {
    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            notebook.next_cell();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            notebook.prev_cell();
        }
        KeyCode::Char('g') => {
            if notebook.last_key == Some('g') {
                notebook.first_cell();
            }
            notebook.last_key = Some('g');
        }
        KeyCode::Char('G') => {
            notebook.last_cell();
        }

        // Cell operations
        KeyCode::Enter => {
            notebook.mode = Mode::Edit;
            notebook.enter_edit_mode();
        }
        KeyCode::Char('a') => {
            notebook.insert_cell_below();
            notebook.mode = Mode::Edit;
        }
        KeyCode::Char('b') => {
            notebook.insert_cell_above();
            notebook.mode = Mode::Edit;
        }
        KeyCode::Char('d') => {
            if notebook.last_key == Some('d') {
                notebook.delete_current_cell();
            }
            notebook.last_key = Some('d');
        }
        KeyCode::Char('y') => {
            if notebook.last_key == Some('y') {
                notebook.yank_cell();
            }
            notebook.last_key = Some('y');
        }
        KeyCode::Char('p') => {
            notebook.paste_cell();
        }

        // Execution
        KeyCode::Char('e') => {
            notebook.execute_current_cell()?;
        }
        KeyCode::Char('E') => {
            notebook.execute_all_cells()?;
        }

        // Cell type
        KeyCode::Char('m') => {
            notebook.set_cell_type(CellType::Markdown);
        }
        KeyCode::Char('c') => {
            notebook.set_cell_type(CellType::Code);
        }

        // Commands
        KeyCode::Char(':') => {
            notebook.mode = Mode::Command;
        }

        // Quit
        KeyCode::Char('q') => {
            notebook.should_quit = true;
        }

        _ => {
            notebook.last_key = None;
        }
    }

    Ok(())
}
```

### 5. Key Bindings (Edit Mode)

```rust
fn handle_edit_mode(notebook: &mut Notebook, key: KeyEvent) -> Result<()> {
    let cell = &mut notebook.cells[notebook.current_cell];

    match key.code {
        KeyCode::Esc => {
            notebook.mode = Mode::Normal;
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+Enter: Execute cell
            notebook.execute_current_cell()?;
            notebook.mode = Mode::Normal;
        }
        _ => {
            // Pass to tui-textarea for editing
            cell.editor.input(key);
        }
    }

    Ok(())
}
```

### 6. Cell Execution

```rust
impl Notebook {
    pub fn execute_current_cell(&mut self) -> Result<()> {
        let cell = &mut self.cells[self.current_cell];

        if cell.cell_type == CellType::Markdown {
            // Just mark as executed, no eval
            cell.execution_count = Some(self.next_execution_count());
            return Ok(());
        }

        // Get code from editor
        let code = cell.editor.lines().join("\n");

        // Parse
        let mut parser = Parser::new(&code)?;
        let expr = parser.parse()?;

        // Execute and measure time
        let start = Instant::now();
        let result = self.evaluator.eval(&expr)?;
        let execution_time = start.elapsed();

        // Store output
        cell.output = Some(CellOutput {
            value: format!("{}", result),
            stdout: String::new(),
            stderr: String::new(),
            execution_time,
        });
        cell.execution_count = Some(self.next_execution_count());
        cell.stale = false;

        Ok(())
    }

    pub fn execute_all_cells(&mut self) -> Result<()> {
        for i in 0..self.cells.len() {
            self.current_cell = i;
            self.execute_current_cell()?;
        }
        Ok(())
    }
}
```

### 7. Syntax Highlighting in Cells

```rust
use tui_textarea::{TextArea, CursorMove};

fn create_code_editor() -> TextArea<'static> {
    let mut textarea = TextArea::default();

    // Set block
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title("Code")
    );

    // Set cursor style
    textarea.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));

    // Line numbers
    textarea.set_line_number_style(Style::default().fg(Color::DarkGray));

    // Syntax highlighting (simple version)
    // For full highlighting, integrate with syntect or tree-sitter
    textarea.set_search_style(Style::default().fg(Color::Yellow));

    textarea
}
```

### 8. Full Layout

```rust
fn render(f: &mut Frame, notebook: &Notebook) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),    // Header
            Constraint::Min(0),       // Cells
            Constraint::Length(1),    // Status bar
        ])
        .split(f.area());

    // Header
    let header = Paragraph::new(format!(
        "Patina Notebook - {} [{}]",
        notebook.filename,
        match notebook.mode {
            Mode::Normal => "NORMAL",
            Mode::Edit => "EDIT",
            Mode::Command => "COMMAND",
        }
    ))
    .style(Style::default().fg(Color::Cyan))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Cells
    let cell_area = chunks[1];
    let cells_per_page = (cell_area.height / 10) as usize; // Estimate
    let visible_cells = &notebook.cells[notebook.scroll..];

    let cell_constraints: Vec<_> = visible_cells
        .iter()
        .take(cells_per_page)
        .map(|_| Constraint::Min(8))
        .collect();

    if !cell_constraints.is_empty() {
        let cell_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(cell_constraints)
            .split(cell_area);

        for (i, chunk) in cell_chunks.iter().enumerate() {
            let cell_idx = notebook.scroll + i;
            if cell_idx < notebook.cells.len() {
                render_cell(
                    f,
                    *chunk,
                    &notebook.cells[cell_idx],
                    cell_idx == notebook.current_cell,
                );
            }
        }
    }

    // Status bar
    let status = Line::from(vec![
        Span::raw(format!("[{}] ", match notebook.mode {
            Mode::Normal => "NORMAL",
            Mode::Edit => "EDIT",
            Mode::Command => "COMMAND",
        })),
        Span::raw(format!("Cell {}/{} | ",
                         notebook.current_cell + 1,
                         notebook.cells.len())),
        Span::raw("Press ? for help"),
    ]);
    let status_bar = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray));
    f.render_widget(status_bar, chunks[2]);
}
```

## Usage

```bash
# Create new notebook
patina notebook new analysis.scm.nb

# Open existing
patina notebook open analysis.scm.nb

# Convert from Jupyter
patina notebook import analysis.ipynb
```

## Example Session

```
┌─────────────────────────────────────────────────────┐
│ Patina Notebook - analysis.scm.nb [NORMAL]         │
├─────────────────────────────────────────────────────┤
│                                                     │
│ ┌─ [1] Code ────────────────────────────────┐      │
│ │ (define data '(1 4 2 8 5 7))              │      │
│ │                                            │      │
│ └────────────────────────────────────────────┘      │
│ Output:                                             │
│ => #<unspecified>                                   │
│                                                     │
│ ┌─ [2] Code ────────────────────────────────┐      │
│ │ (apply + data)▌                            │      │
│ │                                            │      │
│ └────────────────────────────────────────────┘      │
│ Output:                                             │
│ => 27                                               │
│                                                     │
├─────────────────────────────────────────────────────┤
│ [NORMAL] Cell 2/2 | Press ? for help               │
└─────────────────────────────────────────────────────┘
```

## tui-textarea Features We Get

1. **Vim Emulation** - Modal editing out of the box
2. **Yank/Paste** - System clipboard integration
3. **Search** - Regex search within cells
4. **Multi-line** - Natural text editing
5. **Undo/Redo** - Built-in history
6. **Scrolling** - Large cell support

## Next Steps

1. Implement basic TUI with ratatui
2. Integrate tui-textarea for cell editing
3. Add S-expression parser for `.scm.nb`
4. Hook up evaluator
5. Add cell execution tracking
6. Implement save/load
7. Add export functionality

This gives us a terminal notebook that rivals Jupyter in functionality!
