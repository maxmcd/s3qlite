#[cfg(test)]
mod tests {
    use sqlite::{Connection, State};

    unsafe extern "C" {
        fn initialize_grpsqlite() -> i32;
    }

    fn init_vfs() {
        unsafe {
            initialize_grpsqlite();
        }
    }

    #[test]
    fn test_table_creation() {
        init_vfs();
        let connection = Connection::open("test_table_creation.db").unwrap();
        connection
            .execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
    }

    #[test]
    fn test_larger_table() -> sqlite::Result<()> {
        init_vfs();
        let connection = Connection::open("test_larger_table.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, age INTEGER)")?;
        let mut values = String::from("INSERT INTO users (name, email, age) VALUES ");
        for i in 0..100 {
            if i > 0 {
                values.push(',');
            }
            values.push_str(&format!(
                "\n    ('Person {}', 'person{}@email.com', {})",
                i,
                i,
                20 + i
            ));
        }
        values.push(';');
        connection.execute(&values)?;

        let mut stmt = connection.prepare("SELECT name FROM users WHERE id = 1")?;
        assert_eq!(stmt.next()?, State::Row);
        Ok(())
    }

    #[test]
    fn test_concurrent_operations() -> sqlite::Result<()> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;

        init_vfs();
        let db_name = "test_concurrent_ops.db";

        // Initialize database with table
        let connection = Connection::open(db_name)?;
        connection.execute("DROP TABLE IF EXISTS concurrent_users")?;
        connection.execute("CREATE TABLE concurrent_users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, age INTEGER, thread_id INTEGER)")?;
        drop(connection);

        let success_flag = Arc::new(AtomicBool::new(true));
        let mut handles = vec![];

        for thread_id in 0..4 {
            let success_flag_clone = Arc::clone(&success_flag);
            let db_name_clone = db_name.to_string();

            let handle = thread::spawn(move || {
                if run_thread_operations(thread_id, &db_name_clone).is_err() {
                    success_flag_clone.store(false, Ordering::Relaxed);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert!(
            success_flag.load(Ordering::Relaxed),
            "One or more threads failed"
        );

        // Verify final state
        let connection = Connection::open(db_name)?;
        let mut stmt = connection.prepare("SELECT COUNT(*) FROM concurrent_users")?;
        assert_eq!(stmt.next()?, State::Row);
        let total_count: i64 = stmt.read(0)?;

        // Each thread inserts 25 records, some get deleted, so total should be less than 100
        assert!(
            total_count > 0 && total_count <= 100,
            "Expected some records to remain, got {total_count}"
        );

        // Verify each thread has some records
        for thread_id in 0..4 {
            let mut stmt =
                connection.prepare("SELECT COUNT(*) FROM concurrent_users WHERE thread_id = ?")?;
            stmt.bind((1, thread_id))?;
            assert_eq!(stmt.next()?, State::Row);
            let thread_count: i64 = stmt.read(0)?;
            assert!(
                thread_count > 0,
                "Thread {thread_id} should have some records remaining"
            );
        }

        Ok(())
    }

    fn run_thread_operations(thread_id: i64, db_name: &str) -> sqlite::Result<()> {
        let connection = Connection::open(db_name)?;

        // Insert 25 records for this thread
        for i in 0..25 {
            let record_id = thread_id * 100 + i;
            connection.execute(format!(
                "INSERT INTO concurrent_users (name, email, age, thread_id) VALUES ('Person{}', 'person{}@thread{}.com', {}, {})",
                record_id, record_id, thread_id, 20 + i, thread_id
            ))?;
        }

        // Update some records (every 3rd record)
        for i in (0..25).step_by(3) {
            let record_id = thread_id * 100 + i;
            connection.execute(format!(
                "UPDATE concurrent_users SET age = {}, email = 'updated{}@thread{}.com' WHERE name = 'Person{}' AND thread_id = {}",
                30 + i, record_id, thread_id, record_id, thread_id
            ))?;
        }

        // Delete some records (every 5th record)
        for i in (0..25).step_by(5) {
            let record_id = thread_id * 100 + i;
            connection.execute(format!(
                "DELETE FROM concurrent_users WHERE name = 'Person{record_id}' AND thread_id = {thread_id}"
            ))?;
        }

        // Verify our thread's remaining data
        let mut stmt =
            connection.prepare("SELECT COUNT(*) FROM concurrent_users WHERE thread_id = ?")?;
        stmt.bind((1, thread_id))?;
        assert_eq!(stmt.next()?, State::Row);
        let count: i64 = stmt.read(0)?;

        // Should have 25 - (25/5) = 20 records remaining (deleted every 5th)
        let expected_remaining = 25 - (25 / 5);
        assert_eq!(
            count, expected_remaining,
            "Thread {thread_id} should have {expected_remaining} records, got {count}"
        );

        // Verify some updated records exist
        let mut stmt = connection.prepare(
            "SELECT COUNT(*) FROM concurrent_users WHERE thread_id = ? AND email LIKE 'updated%'",
        )?;
        stmt.bind((1, thread_id))?;
        assert_eq!(stmt.next()?, State::Row);
        let updated_count: i64 = stmt.read(0)?;

        // Should have updated records (every 3rd that wasn't deleted)
        assert!(
            updated_count > 0,
            "Thread {thread_id} should have some updated records"
        );

        Ok(())
    }

    #[test]
    fn test_insert_and_select() {
        init_vfs();
        let connection = Connection::open("test_insert_select.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();

        let mut stmt = connection
            .prepare("SELECT name FROM users WHERE id = 1")
            .unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let name: String = stmt.read(0).unwrap();
        assert_eq!(name, "Alice");
    }

    #[test]
    fn test_multiple_inserts() {
        init_vfs();
        let connection = Connection::open("test_multiple_inserts.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Bob')")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Charlie')")
            .unwrap();

        let mut stmt = connection.prepare("SELECT COUNT(*) FROM users").unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let count: i64 = stmt.read(0).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_joins() {
        init_vfs();
        let connection = Connection::open("test_joins.db").unwrap();
        connection.execute("DROP TABLE IF EXISTS users").unwrap();
        connection.execute("DROP TABLE IF EXISTS posts").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT)")
            .unwrap();

        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        connection
            .execute("INSERT INTO posts (user_id, title) VALUES (1, 'Hello World')")
            .unwrap();

        let mut stmt = connection
            .prepare("SELECT u.name, p.title FROM users u JOIN posts p ON u.id = p.user_id")
            .unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let name: String = stmt.read(0).unwrap();
        let title: String = stmt.read(1).unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(title, "Hello World");
    }

    #[test]
    fn test_transactions() {
        init_vfs();
        let connection = Connection::open("test_transactions.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();

        connection.execute("BEGIN TRANSACTION").unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Bob')")
            .unwrap();
        connection.execute("COMMIT").unwrap();

        let mut stmt = connection.prepare("SELECT COUNT(*) FROM users").unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let count: i64 = stmt.read(0).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_updates() {
        init_vfs();
        let connection = Connection::open("test_updates.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT)")
            .unwrap();
        connection
            .execute("INSERT INTO users (name, email) VALUES ('Alice', 'alice@old.com')")
            .unwrap();

        connection
            .execute("UPDATE users SET email = 'alice@new.com' WHERE name = 'Alice'")
            .unwrap();

        let mut stmt = connection
            .prepare("SELECT email FROM users WHERE name = 'Alice'")
            .unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let email: String = stmt.read(0).unwrap();
        assert_eq!(email, "alice@new.com");
    }

    #[test]
    fn test_deletes() {
        init_vfs();
        let connection = Connection::open("test_deletes.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Bob')")
            .unwrap();

        connection
            .execute("DELETE FROM users WHERE name = 'Alice'")
            .unwrap();

        let mut stmt = connection.prepare("SELECT COUNT(*) FROM users").unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let count: i64 = stmt.read(0).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_aggregations() {
        init_vfs();
        let connection = Connection::open("test_aggregations.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT)")
            .unwrap();

        connection
            .execute("INSERT INTO users (name) VALUES ('Alice')")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('Bob')")
            .unwrap();
        connection
            .execute("INSERT INTO posts (user_id, title) VALUES (1, 'Post 1')")
            .unwrap();
        connection
            .execute("INSERT INTO posts (user_id, title) VALUES (1, 'Post 2')")
            .unwrap();
        connection
            .execute("INSERT INTO posts (user_id, title) VALUES (2, 'Post 3')")
            .unwrap();

        let mut stmt = connection.prepare("SELECT u.name, COUNT(p.id) as post_count FROM users u LEFT JOIN posts p ON u.id = p.user_id GROUP BY u.id ORDER BY post_count DESC").unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let name: String = stmt.read(0).unwrap();
        let count: i64 = stmt.read(1).unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_builtin_functions() {
        init_vfs();
        let connection = Connection::open("test_builtin_functions.db").unwrap();
        connection
            .execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        connection
            .execute("INSERT INTO users (name) VALUES ('alice')")
            .unwrap();

        let mut stmt = connection
            .prepare("SELECT UPPER(name), LENGTH(name) FROM users")
            .unwrap();
        assert_eq!(stmt.next().unwrap(), State::Row);
        let upper_name: String = stmt.read(0).unwrap();
        let length: i64 = stmt.read(1).unwrap();
        assert_eq!(upper_name, "ALICE");
        assert_eq!(length, 5);
    }

    #[test]
    fn test_custom_vfs_pragma() {
        init_vfs();
        let connection = Connection::open("test_custom_vfs_pragma.db").unwrap();

        let mut stmt = connection.prepare("PRAGMA is_memory_server").unwrap();
        if stmt.next().unwrap() == State::Row {
            let result: String = stmt.read(0).unwrap();
            println!("result: {result}");
            assert_eq!(result, "maybe?");
        } else {
            panic!("grpsqlite VFS not detected");
        }
    }
}
