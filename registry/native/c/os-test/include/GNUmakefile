include ../misc/GNUmakefile.shared

.PHONY: all
all: test

.PHONY: test
test: $(TEST_RESULTS)

$(OUT_PATH)/%.out: %.c
	../misc/try-compile.sh "$(CC) $(CFLAGS) $(CPPFLAGS)" $* "$(LDFLAGS)" "$(EXTRA_LDFLAGS)" "$(OS)" "$(OUT_PATH)"

.PHONY: clean
clean: clean-test

.PHONY: clean-test
clean-test:
	rm -rf $(OUT_PATH)
