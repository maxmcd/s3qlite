use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use sqlite::{Connection, State};
use std::process;

mod main_test;

unsafe extern "C" {
    fn initialize_grpsqlite() -> i32;
}

struct SqliteRepl {
    connection: Option<Connection>,
    current_db: String,
}

impl SqliteRepl {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        // Initialize grpsqlite VFS
        println!("Initializing grpsqlite VFS...");
        unsafe { initialize_grpsqlite() };

        // Open database connection
        let connection = Connection::open("repl.db")?;

        // Test VFS with pragma
        let mut vfs_detected = false;
        if let Ok(mut stmt) = connection.prepare("PRAGMA is_memory_server") {
            if let Ok(State::Row) = stmt.next() {
                if let Ok(result) = stmt.read::<String, _>(0) {
                    println!("result: {result}");
                    vfs_detected = result == "maybe?";
                }
            }
        }

        if !vfs_detected {
            return Err("grpsqlite VFS not detected".into());
        }

        Ok(Self {
            connection: None,
            current_db: "repl.db".to_string(),
        })
    }

    fn prompt(&self) -> String {
        let db_name = std::path::Path::new(&self.current_db)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("db");

        format!("sql:{db_name}> ")
    }

    fn execute_meta_command(&mut self, command: &str) -> bool {
        match command.trim().to_lowercase().as_str() {
            ".help" => {
                println!("\nAvailable commands:");
                println!("  .help           Show this help");
                println!("  .quit           Exit the REPL");
                println!("  .exit           Exit the REPL");
                println!("  .open <file>    Open a database file");
                println!("  .tables         List all tables");
                println!("  .schema [table] Show table schema");
                println!("\nEnter SQL statements to execute them.");
                println!("Use semicolon (;) to end statements.");
            }
            ".quit" | ".exit" => {
                println!("Goodbye!");
                return false;
            }
            ".tables" => {
                self.list_tables();
            }
            cmd if cmd.starts_with(".open") => {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.len() > 1 {
                    self.open_database(parts[1]);
                } else {
                    println!("Usage: .open <filename>");
                }
            }
            cmd if cmd.starts_with(".schema") => {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.len() > 1 {
                    self.show_schema(parts[1]);
                } else {
                    self.show_all_schemas();
                }
            }
            _ => {
                println!("Unknown command: {command}");
                println!("Type .help for available commands");
            }
        }
        true
    }

    fn open_database(&mut self, filename: &str) {
        match Connection::open(filename) {
            Ok(new_connection) => {
                self.connection = Some(new_connection);
                self.current_db = filename.to_string();
                println!("Opened database: {filename}");
            }
            Err(e) => {
                println!("Failed to open database '{filename}': {e}");
            }
        }
    }

    fn list_tables(&self) {
        if self.connection.is_none() {
            println!("No database opened");
            return;
        }

        match self
            .connection
            .as_ref()
            .unwrap()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        {
            Ok(mut stmt) => {
                println!("\nTables:");
                let mut found_any = false;
                while let Ok(State::Row) = stmt.next() {
                    if let Ok(name) = stmt.read::<String, _>(0) {
                        println!("  {name}");
                        found_any = true;
                    }
                }
                if !found_any {
                    println!("  No tables found");
                }
                println!();
            }
            Err(e) => {
                println!("Error listing tables: {e}");
            }
        }
    }

    fn show_schema(&self, table_name: &str) {
        if self.connection.is_none() {
            println!("No database opened");
            return;
        }
        match self.connection.as_ref().unwrap().prepare(format!(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='{table_name}'"
        )) {
            Ok(mut stmt) => {
                if let Ok(State::Row) = stmt.next() {
                    if let Ok(schema) = stmt.read::<String, _>(0) {
                        println!("\nSchema for table '{table_name}':");
                        println!("{schema}");
                        println!();
                    }
                } else {
                    println!("Table '{table_name}' not found");
                }
            }
            Err(e) => {
                println!("Error getting schema: {e}");
            }
        }
    }

    fn show_all_schemas(&self) {
        if self.connection.is_none() {
            println!("No database opened");
            return;
        }
        match self
            .connection
            .as_ref()
            .unwrap()
            .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' ORDER BY name")
        {
            Ok(mut stmt) => {
                println!("\nAll table schemas:");
                let mut found_any = false;
                while let Ok(State::Row) = stmt.next() {
                    if let (Ok(name), Ok(schema)) =
                        (stmt.read::<String, _>(0), stmt.read::<String, _>(1))
                    {
                        println!("\n{name}:");
                        println!("{schema}");
                        found_any = true;
                    }
                }
                if !found_any {
                    println!("  No tables found");
                }
                println!();
            }
            Err(e) => {
                println!("Error getting schemas: {e}");
            }
        }
    }

    fn execute_sql(&self, sql: &str) -> bool {
        let sql = sql.trim();
        if sql.is_empty() {
            return true;
        }

        // Check if it's a SELECT query
        if sql.to_lowercase().starts_with("select") {
            self.execute_select(sql)
        } else {
            self.execute_non_select(sql)
        }
    }

    fn execute_select(&self, sql: &str) -> bool {
        if self.connection.is_none() {
            println!("No database opened");
            return false;
        }
        match self.connection.as_ref().unwrap().prepare(sql) {
            Ok(mut stmt) => {
                // Get column names
                let column_count = stmt.column_count();
                let mut column_names = Vec::new();
                for i in 0..column_count {
                    column_names.push(stmt.column_name(i).unwrap_or("").to_string());
                }

                // Collect all rows
                let mut rows = Vec::new();
                while let Ok(State::Row) = stmt.next() {
                    let mut row = Vec::new();
                    for i in 0..column_count {
                        let value = stmt
                            .read::<String, _>(i)
                            .unwrap_or_else(|_| "NULL".to_string());
                        row.push(value);
                    }
                    rows.push(row);
                }

                if rows.is_empty() {
                    println!("No rows returned.");
                } else {
                    self.print_table(&column_names, &rows);
                }
                true
            }
            Err(e) => {
                println!("SQL Error: {e}");
                true
            }
        }
    }

    fn execute_non_select(&self, sql: &str) -> bool {
        if self.connection.is_none() {
            println!("No database opened");
            return false;
        }
        match self.connection.as_ref().unwrap().execute(sql) {
            Ok(()) => {
                println!("Query executed successfully.");
                true
            }
            Err(e) => {
                println!("SQL Error: {e}");
                true
            }
        }
    }

    fn print_table(&self, columns: &[String], rows: &[Vec<String>]) {
        if columns.is_empty() || rows.is_empty() {
            return;
        }

        // Calculate column widths
        let mut widths = columns.iter().map(|c| c.len()).collect::<Vec<_>>();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.len());
            }
        }

        // Print header
        print!("┌");
        for (i, width) in widths.iter().enumerate() {
            print!("{}", "─".repeat(width + 2));
            if i < widths.len() - 1 {
                print!("┬");
            }
        }
        println!("┐");

        print!("│");
        for (column, width) in columns.iter().zip(widths.iter()) {
            print!(" {column:<width$} ");
            print!("│");
        }
        println!();

        print!("├");
        for (i, width) in widths.iter().enumerate() {
            print!("{}", "─".repeat(width + 2));
            if i < widths.len() - 1 {
                print!("┼");
            }
        }
        println!("┤");

        // Print rows
        for row in rows {
            print!("│");
            for (cell, width) in row.iter().zip(widths.iter()) {
                print!(" {cell:<width$} ");
                print!("│");
            }
            println!();
        }

        print!("└");
        for (i, width) in widths.iter().enumerate() {
            print!("{}", "─".repeat(width + 2));
            if i < widths.len() - 1 {
                print!("┴");
            }
        }
        println!("┘");

        println!("({} rows)", rows.len());
    }

    fn run(&mut self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut rl = DefaultEditor::new()?;

        // Try to load history
        let _ = rl.load_history("repl_history.txt");

        println!("\nWelcome to grpsqlite REPL!");
        println!("Type .help for commands or enter SQL statements.");
        println!();

        let mut buffer = String::new();
        let mut in_multiline = false;

        loop {
            let prompt = if in_multiline {
                "     ...> ".to_string()
            } else {
                self.prompt()
            };

            match rl.readline(&prompt) {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    // Add to history
                    let _ = rl.add_history_entry(line.as_str());

                    // Handle meta commands (only if not in multiline mode)
                    if !in_multiline && line.trim().starts_with('.') {
                        if !self.execute_meta_command(&line) {
                            break;
                        }
                        continue;
                    }

                    // Accumulate SQL statement
                    if in_multiline {
                        buffer.push(' ');
                    }
                    buffer.push_str(&line);

                    // Check if statement is complete (ends with semicolon)
                    if line.trim().ends_with(';') {
                        self.execute_sql(&buffer);
                        buffer.clear();
                        in_multiline = false;
                    } else {
                        in_multiline = true;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("Use .quit to exit");
                }
                Err(ReadlineError::Eof) => {
                    println!("\nGoodbye!");
                    break;
                }
                Err(err) => {
                    println!("Error: {err:?}");
                    break;
                }
            }
        }

        // Save history
        let _ = rl.save_history("repl_history.txt");
        Ok(())
    }
}

fn main() {
    match SqliteRepl::new() {
        Ok(mut repl) => {
            if let Err(e) = repl.run() {
                eprintln!("REPL error: {e}");
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to initialize REPL: {e}");
            process::exit(1);
        }
    }
}
