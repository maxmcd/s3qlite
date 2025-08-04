SQLITE_YEAR = 2025
SQLITE_FILENAME_VERSION = 3490200
SQLITE_TARBALL_FILENAME = sqlite-autoconf-$(SQLITE_FILENAME_VERSION).tar.gz
SQLITE_URL = https://www.sqlite.org/$(SQLITE_YEAR)/$(SQLITE_TARBALL_FILENAME)

# Detect OS for platform-specific settings
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    LIB_EXT = dylib
    LIB_NAME = libsqlite3.$(LIB_EXT)
    SONAME_FLAG = -Wl,-install_name,$(LIB_NAME)
    WHOLE_ARCHIVE = -Wl,-force_load,target/debug/libs3qlite.a
    PLATFORM_LIBS = -framework Security -framework CoreFoundation
    DEPLOYMENT_TARGET = MACOSX_DEPLOYMENT_TARGET=15.4
else
    LIB_EXT = so
    LIB_NAME = libsqlite3.$(LIB_EXT)
    SONAME_FLAG = -Wl,-soname,$(LIB_NAME).1
    WHOLE_ARCHIVE = -Wl,--whole-archive target/debug/libs3qlite.a -Wl,--no-whole-archive
    PLATFORM_LIBS =
    DEPLOYMENT_TARGET =
endif

.PHONY: download-sqlite clean build-sqlite setup-repl

sqlite: $(SQLITE_TARBALL_FILENAME)
	tar xvfz $(SQLITE_TARBALL_FILENAME)
	mv sqlite-autoconf-$(SQLITE_FILENAME_VERSION) sqlite
	rm -f $(SQLITE_TARBALL_FILENAME)

$(SQLITE_TARBALL_FILENAME):
	wget $(SQLITE_URL)

download-sqlite: sqlite

compile-dylib:
	cargo build

setup-repl: build-sqlite
	mkdir -p repl/lib repl/pkgconfig repl/.cargo
	# Copy your custom library as libsqlite3
	cp $(LIB_NAME) repl/lib/libsqlite3.$(LIB_EXT)
	# Copy headers
	cp sqlite/sqlite3.h repl/lib/
	cp sqlite/sqlite3ext.h repl/lib/

repl-run: setup-repl
	cd repl && env DYLD_LIBRARY_PATH=$$(pwd)/lib:$$DYLD_LIBRARY_PATH cargo run

clean:
	rm -rf sqlite $(SQLITE_TARBALL_FILENAME) $(LIB_NAME) repl/lib repl/.cargo repl/pkgconfig

build-sqlite: sqlite compile-dylib
	$(DEPLOYMENT_TARGET) clang -shared -o $(LIB_NAME) \
		$(SONAME_FLAG) \
		-I. \
		-DSQLITE_ENABLE_COLUMN_METADATA=1 \
		-DSQLITE_ENABLE_LOAD_EXTENSION=1 \
		-DSQLITE_ENABLE_FTS5=1 \
		-DSQLITE_ENABLE_BATCH_ATOMIC_WRITE=1 \
		-DSQLITE_ENABLE_DBSTAT_VTAB=1 \
		-DSQLITE_ENABLE_NULL_TRIM=1 \
		-DSQLITE_ENABLE_RTREE=1 \
		-DHAVE_READLINE=0 \
		-D_GNU_SOURCE \
		-O2 \
		-fPIC \
		sqlite/sqlite3.c  \
		$(WHOLE_ARCHIVE) \
		-lpthread -ldl -lm $(PLATFORM_LIBS)
