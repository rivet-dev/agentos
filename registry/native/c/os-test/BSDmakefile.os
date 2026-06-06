OS_LIST != ls os
SUITE_LIST != cat misc/suites.list

CC_FOR_BUILD ?= $(CC)
CFLAGS_FOR_BUILD ?= $(CFLAGS)
CPPFLAGS_FOR_BUILD ?= $(CPPFLAGS)
LDFLAGS_FOR_BUILD ?= $(LDFLAGS) $(EXTRA_LDFLAGS)

.PHONY: all
all: test

.PHONY: test
test: $(OS_LIST)

.for OS in $(OS_LIST)
.PHONY: $(OS)
$(OS):
	mkdir -p tmp
	rm -rf 'tmp/$(OS)'
	mkdir -p 'tmp/$(OS)'
	cp -R -t 'tmp/$(OS)' -- Makefile BSDmakefile GNUmakefile misc $(SUITE_LIST)
	echo $(SUITE_LIST) | tr ' ' '\n' > 'tmp/$(OS)/misc/suites.list'
	$(MAKE) -C tmp/$* clean
	cd 'tmp/$(OS)' && '../../os/$(OS)'
	mkdir -p out
	rm -rf 'out/$(OS)'
	mv 'tmp/$(OS)/out/'* 'out/$(OS)'
	rm -rf 'tmp/$(OS)'

.PHONY: $(OS)-clean
$(OS)-clean: os/$(OS)
	rm -rf 'out/$(OS)'
.endfor

.PHONY: clean
clean:
	rm -rf out
	rm -rf html
	rm -rf tmp
	rm -f misc/genbasic
	rm -f misc/html
	rm -f misc/namespace
	rm -f os-test.json
	rm -f os-test.jsonl

.PHONY: distclean
distclean: clean

misc/html: misc/html.c
	$(CC_FOR_BUILD) $(CFLAGS_FOR_BUILD) $(CPPFLAGS_FOR_BUILD) misc/html.c -o $@ $(LDFLAGS_FOR_BUILD)

.PHONY: html
html: test misc/html
	misc/html --os-list="$(OS_LIST)" --suites-list="$(SUITE_LIST)"

.PHONY: json
json: os-test.json

os-test.json: test misc/html
	misc/html --os-list="$(OS_LIST)" --suites-list="$(SUITE_LIST)" --format=json --output=os-test.json

.PHONY: jsonl
jsonl: os-test.jsonl

os-test.jsonl: test misc/html
	misc/html --os-list="$(OS_LIST)" --suites-list="$(SUITE_LIST)" --format=jsonl --output=os-test.jsonl
