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
        let _ = connection.execute("DROP TABLE IF EXISTS test");
        connection
            .execute("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
    }

    #[test]
    fn test_insert_and_select() {
        init_vfs();
        let connection = Connection::open("test_insert_select.db").unwrap();
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
        let _ = connection.execute("DROP TABLE IF EXISTS posts");
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
        let _ = connection.execute("DROP TABLE IF EXISTS users");
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
