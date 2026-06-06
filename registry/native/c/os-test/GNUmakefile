SUITE_LIST = $(shell cat misc/suites.list)
OS := $(shell uname -s)
OUT_DIRECTORY ?= out
OUT_OS := $(shell echo "$(OS)" | tr '[:upper:]' '[:lower:]')
OUT_PATH = $(OUT_DIRECTORY)/$(OUT_OS)

CC_FOR_BUILD ?= $(CC)
CFLAGS_FOR_BUILD ?= $(CFLAGS)
CPPFLAGS_FOR_BUILD ?= $(CPPFLAGS)
LDFLAGS_FOR_BUILD ?= $(LDFLAGS) $(EXTRA_LDFLAGS)

.PHONY: all
all: $(SUITE_LIST:=-all)

%-all: %/Makefile
	$(MAKE) -C $*

$(OUT_PATH)/uname.out:
	mkdir -p $(OUT_PATH)
	misc/uname.sh > $(OUT_PATH)/uname.out

.PHONY: clean
test: $(OUT_PATH)/uname.out $(SUITE_LIST:=-test)

%-test: %/Makefile
	$(MAKE) -C $* test

.PHONY: clean
clean: $(SUITE_LIST:=-clean) 
	rm -rf tmp
	rm -f misc/genbasic
	rm -f misc/html
	rm -f misc/namespace

.PHONY: clean-html
clean-html:
	rm -rf html

.PHONY: clean-json
clean-json:
	rm -rf os-test.json
	rm -rf os-test.jsonl

%-clean: %/Makefile
	$(MAKE) -C $* clean

.PHONY: clean-test
clean-test: clean
	rm -rf $(OUT_DIRECTORY)

%-clean-test: %/Makefile
	$(MAKE) -C $* clean-test

.PHONY: clean-susv5
clean-susv5:
	rm -rf susv5.tgz susv5-html

.PHONY: distclean
distclean: clean clean-test clean-html clean-json posix-parse-clean clean-susv5

misc/html: misc/html.c
	$(CC_FOR_BUILD) $(CFLAGS_FOR_BUILD) $(CPPFLAGS_FOR_BUILD) misc/html.c -o $@ $(LDFLAGS_FOR_BUILD)

.PHONY: html
html: test misc/html
	misc/html --suites-list="$(SUITE_LIST)"

.PHONY: json
json: os-test.json

os-test.json: test misc/html
	misc/html --suites-list="$(SUITE_LIST)" --format=json --output=os-test.json

.PHONY: jsonl
jsonl: os-test.jsonl

os-test.jsonl: test misc/html
	misc/html --suites-list="$(SUITE_LIST)" --format=jsonl --output=os-test.jsonl
