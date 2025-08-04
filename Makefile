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
else
    EXT = so
    SONAME = -Wl,-soname,libsqlite3.$(EXT).1
    ARCHIVE_FLAGS = -Wl,--whole-archive target/debug/libs3qlite.a -Wl,--no-whole-archive
    LIBS =
    ENV =
endif

LIB = libsqlite3.$(EXT)
RUST_LIB = target/debug/libs3qlite.a

.PHONY: clean repl

all: $(LIB)

sqlite/sqlite3.c:
	wget $(SQLITE_URL)
	tar xf $(SQLITE_ARCHIVE)
	mv sqlite-autoconf-$(SQLITE_VERSION) sqlite
	rm $(SQLITE_ARCHIVE)

$(RUST_LIB): src/**/*.rs src/*.rs Cargo.lock Cargo.toml
	env RUSTFLAGS="-L /opt/homebrew/lib -l sqlite3" cargo build

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
		-O2 -fPIC \
		sqlite/sqlite3.c $(ARCHIVE_FLAGS) \
		-lpthread -ldl -lm $(LIBS)

repl/lib/$(LIB): $(LIB) sqlite/sqlite3.h sqlite/sqlite3ext.h | repl/lib
	cp $(LIB) $@
	cp sqlite/sqlite3.h sqlite/sqlite3ext.h repl/lib/

repl/lib:
	mkdir -p $@

repl: repl/lib/$(LIB)
	cd repl && env DYLD_LIBRARY_PATH=$$(pwd)/lib:$$DYLD_LIBRARY_PATH cargo run

clean:
	cargo clean
	rm -rf sqlite $(SQLITE_ARCHIVE) $(LIB) repl/lib
