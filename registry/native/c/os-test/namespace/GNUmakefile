include ../misc/GNUmakefile.shared

.PHONY: all
all: test

.PHONY: test
test: $(TEST_RESULTS)

.SUFFIXES: .i .c .dM

../misc/namespace: ../misc/namespace.c
	$(CC_FOR_BUILD) $(CFLAGS_FOR_BUILD) $(CPPFLAGS_FOR_BUILD) ../misc/namespace.c -o $@ $(LDFLAGS_FOR_BUILD)

.c.i:
	$(CC) $(CFLAGS) $(CPPFLAGS) -D_POSIX_C_SOURCE=202405L $(STD_C17) -E $< -o $@ 2>/dev/null || echo missing_header > $@

.c.dM:
	$(CC) $(CFLAGS) $(CPPFLAGS) -D_POSIX_C_SOURCE=202405L $(STD_C17) -E -dM $< -o $@ 2>/dev/null || echo missing_header > $@

$(TEST_RESULTS): ../misc/namespace

$(OUT_PATH)/%-xsi.out: %-xsi.i %-xsi.dM
	../misc/namespace -o $@ -e $(@:.out=.err) -I ../include -i $* -x $*-xsi.i $*-xsi.dM

$(OUT_PATH)/%.out: %.i %.dM
	../misc/namespace -o $@ -e $(@:.out=.err) -I ../include -i $* $*.i $*.dM

.PHONY: clean
clean: clean-test
	rm -f *.i *.dM
	rm -f ../misc/namespace

.PHONY: clean-test
clean-test:
	rm -rf $(OUT_PATH)
