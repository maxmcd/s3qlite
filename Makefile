SQLITE_VERSION = 3490200
SQLITE_YEAR = 2025
SQLITE_ARCHIVE = sqlite-autoconf-$(SQLITE_VERSION).tar.gz
SQLITE_URL = https://www.sqlite.org/$(SQLITE_YEAR)/$(SQLITE_ARCHIVE)

UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    EXT = dylib
    SONAME = -Wl,-install_name,libsqlite3.$(EXT)
    ARCHIVE_FLAGS = -Wl,-force_load,target/debug/libs3qlite.a
    LIBS = -framework Security -framework CoreFoundation
    ENV = MACOSX_DEPLOYMENT_TARGET=15.4
    RUST_ENV = RUSTFLAGS="-L /opt/homebrew/lib -l sqlite3"
    BUILD_ENV = LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib
else
    EXT = so
    SONAME = -Wl,-soname,libsqlite3.$(EXT).1
    ARCHIVE_FLAGS = -Wl,--whole-archive target/debug/libs3qlite.a -Wl,--no-whole-archive
    LIBS =
    ENV =
    RUST_ENV =
    BUILD_ENV = LIBCLANG_PATH=/usr/lib/llvm-18/lib
endif

LIB = libsqlite3.$(EXT)
STATIC_LIB = libsqlite3.a
SQLITE_OBJ = sqlite3.o
RUST_LIB = target/debug/libs3qlite.a

.PHONY: clean repl repl-static static

all: $(LIB)

static: $(STATIC_LIB)

sqlite/sqlite3.c:
	wget $(SQLITE_URL)
	tar xf $(SQLITE_ARCHIVE)
	mv sqlite-autoconf-$(SQLITE_VERSION) sqlite
	rm $(SQLITE_ARCHIVE)

$(RUST_LIB): src/**/*.rs src/*.rs Cargo.lock Cargo.toml
	env $(BUILD_ENV) $(RUST_ENV) cargo build

$(LIB): sqlite/sqlite3.c $(RUST_LIB)
	$(ENV) clang -shared -o $@ $(SONAME) -I. \
		-DSQLITE_ENABLE_COLUMN_METADATA=1 \
		-DSQLITE_ENABLE_LOAD_EXTENSION=1 \
		-DSQLITE_ENABLE_FTS5=1 \
		-DSQLITE_ENABLE_BATCH_ATOMIC_WRITE=1 \
		-DSQLITE_ENABLE_DBSTAT_VTAB=1 \
		-DSQLITE_ENABLE_NULL_TRIM=1 \
		-DSQLITE_ENABLE_RTREE=1 \
		-DHAVE_READLINE=0 \
		-D_GNU_SOURCE \
		-O2 -g -fPIC \
		sqlite/sqlite3.c $(ARCHIVE_FLAGS) \
		-lpthread -ldl -lm $(LIBS)

$(SQLITE_OBJ): sqlite/sqlite3.c
	$(ENV) clang -c -o $@ -I. \
		-DSQLITE_ENABLE_COLUMN_METADATA=1 \
		-DSQLITE_ENABLE_LOAD_EXTENSION=1 \
		-DSQLITE_ENABLE_FTS5=1 \
		-DSQLITE_ENABLE_BATCH_ATOMIC_WRITE=1 \
		-DSQLITE_ENABLE_DBSTAT_VTAB=1 \
		-DSQLITE_ENABLE_NULL_TRIM=1 \
		-DSQLITE_ENABLE_RTREE=1 \
		-DHAVE_READLINE=0 \
		-D_GNU_SOURCE \
		-O2 -g \
		sqlite/sqlite3.c

$(STATIC_LIB): $(SQLITE_OBJ) $(RUST_LIB)
	rm $@ || true # delete the old version, otherwise it will be re-used
	mkdir -p temp_static
	cd temp_static && ar x ../$(RUST_LIB)
	ar rcs $@ $(SQLITE_OBJ) temp_static/*.o
	rm -rf temp_static

repl/lib/$(LIB): $(LIB) sqlite/sqlite3.h sqlite/sqlite3ext.h | repl/lib
	cp $(LIB) $@
	cp sqlite/sqlite3.h sqlite/sqlite3ext.h repl/lib/
ifeq ($(UNAME_S),Linux)
	cd repl/lib && ln -sf $(LIB) $(LIB).1
endif

repl/lib/$(STATIC_LIB): $(STATIC_LIB) sqlite/sqlite3.h sqlite/sqlite3ext.h | repl/lib
	cp $(STATIC_LIB) $@
	cp sqlite/sqlite3.h sqlite/sqlite3ext.h repl/lib/

repl/lib:
	mkdir -p $@

repl: repl/lib/$(STATIC_LIB)
	cd repl && cargo run

build: repl/lib/$(STATIC_LIB)
	cd repl && cargo build --release

test: repl/lib/$(STATIC_LIB)
	cd repl && cargo test --package repl --bin repl -- main_test::tests::test_concurrent_operations --exact --show-output

clean:
	cargo clean
	cd repl && cargo clean
	rm -rf sqlite $(SQLITE_ARCHIVE) $(LIB) $(STATIC_LIB) $(SQLITE_OBJ) repl/lib
